use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, LazyLock};

struct TrackingAlloc;

static CURRENT_ALLOCS: AtomicU64 = AtomicU64::new(0);
static PEAK_ALLOCS: AtomicU64 = AtomicU64::new(0);
static CURRENT_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_BYTES: AtomicU64 = AtomicU64::new(0);

static ALLOC_MAP: LazyLock<Mutex<HashMap<usize, usize>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let size = layout.size();
            CURRENT_ALLOCS.fetch_add(1, Ordering::Relaxed);
            let cur_bytes = CURRENT_BYTES.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
            // update peak allocs
            update_peak(&PEAK_ALLOCS, CURRENT_ALLOCS.load(Ordering::Relaxed));
            update_peak(&PEAK_BYTES, cur_bytes);
            if let Ok(mut m) = ALLOC_MAP.lock() {
                m.insert(ptr as usize, size);
            }
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // consult map to find the originally recorded size if available
        let mut removed = None;
        if let Ok(mut m) = ALLOC_MAP.lock() {
            removed = m.remove(&(ptr as usize));
        }
        let size = removed.unwrap_or(layout.size());
        System.dealloc(ptr, layout);
        CURRENT_ALLOCS.fetch_sub(1, Ordering::Relaxed);
        CURRENT_BYTES.fetch_sub(size as u64, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            // remove old mapping if present
            let mut old_size = layout.size();
            if let Ok(mut m) = ALLOC_MAP.lock() {
                if let Some(sz) = m.remove(&(ptr as usize)) {
                    old_size = sz;
                }
                m.insert(new_ptr as usize, new_size);
            }
            // adjust counters: subtract old, add new
            if new_size as u64 >= old_size as u64 {
                let added = new_size as u64 - old_size as u64;
                let cur = CURRENT_BYTES.fetch_add(added, Ordering::Relaxed) + added;
                update_peak(&PEAK_BYTES, cur);
            } else {
                let removed = old_size as u64 - new_size as u64;
                CURRENT_BYTES.fetch_sub(removed, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static GLOBAL: TrackingAlloc = TrackingAlloc;

fn update_peak(a: &AtomicU64, candidate: u64) {
    let mut prev = a.load(Ordering::Relaxed);
    while candidate > prev {
        match a.compare_exchange(prev, candidate, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(v) => prev = v,
        }
    }
}

fn reset_alloc_counters() {
    CURRENT_ALLOCS.store(0, Ordering::Relaxed);
    PEAK_ALLOCS.store(0, Ordering::Relaxed);
    CURRENT_BYTES.store(0, Ordering::Relaxed);
    PEAK_BYTES.store(0, Ordering::Relaxed);
    if let Ok(mut m) = ALLOC_MAP.lock() {
        m.clear();
    }
}

fn snapshot_alloc_counters() -> (u64, u64, u64, u64) {
    (
        CURRENT_ALLOCS.load(Ordering::Relaxed),
        CURRENT_BYTES.load(Ordering::Relaxed),
        PEAK_ALLOCS.load(Ordering::Relaxed),
        PEAK_BYTES.load(Ordering::Relaxed),
    )
}
use datapath::socket::{SliceRing, XdpSocket};
use datapath::Forwarder;
use routing::{RouteEntry, Table};
use std::time::SystemTime;
use wire::{Header, HEADER_SIZE};

struct SliceSocket {
    templates: Vec<Vec<u8>>,
}

impl SliceSocket {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            templates: Vec::with_capacity(capacity),
        }
    }

    fn reset(&mut self, frames: &[Vec<u8>]) {
        self.templates.clear();
        self.templates.extend(frames.iter().cloned());
    }
}

impl XdpSocket for SliceSocket {
    fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> {
        self.templates.clone()
    }

    fn poll_slices(&mut self, _max: usize, ring: &mut SliceRing) -> usize {
        ring.clear();
        for template in &self.templates {
            let idx = ring.claim();
            let slot = ring.slot_mut(idx);
            let len = template.len().min(slot.len());
            slot[..len].copy_from_slice(&template[..len]);
            ring.set_len(idx, len);
            ring.active.push(idx);
        }
        ring.active.len()
    }

    fn send(&mut self, _buf: &[u8], _offsets: &[(usize, usize)]) -> Result<(), ()> {
        Ok(())
    }
}

fn build_packet(payload_len: usize, seq: u64) -> Vec<u8> {
    let mut buf = Header::new_header_buffer(payload_len);
    let header = Header {
        src_id: [1u8; 32],
        dst_id: [2u8; 32],
        flow_label: 0x1,
        seq_num: seq,
        session_id: [0u8; 16],
        flags: 0,
        length: payload_len as u16,
    };
    header.marshal_into(&mut buf).unwrap();
    for (index, byte) in buf[HEADER_SIZE..].iter_mut().enumerate() {
        *byte = (index & 0xFF) as u8;
    }
    buf
}

fn build_miss_packet(payload_len: usize, seq: u64) -> Vec<u8> {
    let mut buf = Header::new_header_buffer(payload_len);
    let header = Header {
        src_id: [1u8; 32],
        dst_id: [9u8; 32],
        flow_label: 0x1,
        seq_num: seq,
        session_id: [0u8; 16],
        flags: 0,
        length: payload_len as u16,
    };
    header.marshal_into(&mut buf).unwrap();
    for (index, byte) in buf[HEADER_SIZE..].iter_mut().enumerate() {
        *byte = (index & 0xFF) as u8;
    }
    buf
}

fn build_forwarder() -> Forwarder {
    let routes = Table::new();
    routes.update_route(RouteEntry {
        dest_id: [2u8; 32],
        next_hop_id: [3u8; 32],
        metric: 1,
        last_seen: SystemTime::now(),
    });
    Forwarder::new(routes)
}

struct DatapathFixture {
    forwarder: Forwarder,
    templates: Vec<Vec<u8>>,
    socket: SliceSocket,
    ring: SliceRing,
}

impl DatapathFixture {
    fn new(packet_count: usize, payload_len: usize, miss: bool) -> Self {
        let forwarder = build_forwarder();
        let packet_len = HEADER_SIZE + payload_len;
        let templates = (0..packet_count)
            .map(|seq| {
                if miss {
                    build_miss_packet(payload_len, seq as u64)
                } else {
                    build_packet(payload_len, seq as u64)
                }
            })
            .collect::<Vec<_>>();
        let socket = SliceSocket::with_capacity(packet_count);
        let ring = SliceRing::new(packet_count * 4, packet_len);

        Self {
            forwarder,
            templates,
            socket,
            ring,
        }
    }

    fn run(&mut self) {
        self.socket.reset(&self.templates);
        self.forwarder
            .process_batch_slices(&mut self.socket, &mut self.ring);
    }
}

fn datapath_forwarder_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("datapath_forwarder");
    let packet_counts = [16usize, 64, 256];
    let payload_sizes = [64usize, 128, 256, 512, 1024];

    for &packet_count in &packet_counts {
        for &payload_len in &payload_sizes {
            group.throughput(Throughput::Elements(packet_count as u64));
            group.bench_with_input(
                format!("packets_{}_payload_{}", packet_count, payload_len),
                &(packet_count, payload_len),
                |b, &(count, payload)| {
                    let mut fixture = DatapathFixture::new(count, payload, false);
                    b.iter(|| fixture.run());
                },
            );
        }
    }

    group.finish();
}

fn datapath_forwarder_miss_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("datapath_forwarder_miss");
    let packet_counts = [16usize, 64, 256];
    let payload_sizes = [64usize, 128, 256, 512, 1024];

    for &packet_count in &packet_counts {
        for &payload_len in &payload_sizes {
            group.throughput(Throughput::Elements(packet_count as u64));
            group.bench_with_input(
                format!("packets_{}_payload_{}", packet_count, payload_len),
                &(packet_count, payload_len),
                |b, &(count, payload)| {
                    let mut fixture = DatapathFixture::new(count, payload, true);
                    b.iter(|| fixture.run());
                },
            );
        }
    }

    group.finish();
}

fn datapath_alloc_tracking_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("datapath_alloc_events");
    let packet_counts = [16usize, 64, 256];
    let payload_sizes = [64usize, 128, 256, 512, 1024];

    for &packet_count in &packet_counts {
        for &payload_len in &payload_sizes {
            group.bench_with_input(
                format!("allocs_packets_{}_payload_{}", packet_count, payload_len),
                &(packet_count, payload_len),
                |b, &(count, payload)| {
                    let mut fixture = DatapathFixture::new(count, payload, false);
                    // Run a single controlled iteration to capture peak/current alloc stats
                    reset_alloc_counters();
                    fixture.run();
                    let (calls, bytes, peak_calls, peak_bytes) = snapshot_alloc_counters();
                    // ensure directory exists and append CSV line
                    // Ensure bench_results dir and append CSV line so runs can capture allocation peaks
                    if let Err(e) = std::fs::create_dir_all("tools/bench_results") {
                        eprintln!("failed to create bench_results dir: {}", e);
                    }
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("tools/bench_results/datapath_alloc_peaks.csv")
                    {
                        use std::io::Write;
                        let _ = writeln!(
                            f,
                            "packets,{},payload,{},current_allocs,{},current_bytes,{},peak_allocs,{},peak_bytes,{}",
                            count, payload, calls, bytes, peak_calls, peak_bytes
                        );
                    }
                    // Now run the timing benchmark iterations as usual
                    b.iter(|| {
                        reset_alloc_counters();
                        fixture.run();
                        let (calls, bytes, peak_calls, peak_bytes) = snapshot_alloc_counters();
                        std::hint::black_box((calls, bytes, peak_calls, peak_bytes));
                    });
                },
            );
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    datapath_forwarder_benchmark,
    datapath_forwarder_miss_benchmark,
    datapath_alloc_tracking_benchmark
    ,
    datapath_alloc_tracking_benchmark
);
criterion_main!(benches);
