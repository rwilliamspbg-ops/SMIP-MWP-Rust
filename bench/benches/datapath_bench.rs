use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

struct CountingAlloc;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        ALLOC_COUNT.fetch_add(0, Ordering::Relaxed); // keep count of allocs only
        // We avoid subtracting bytes on dealloc to keep accounting simple and monotonic
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr2 = System.realloc(ptr, layout, new_size);
        if !ptr2.is_null() {
            ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        ptr2
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn reset_alloc_counters() {
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
}

fn snapshot_alloc_counters() -> (u64, u64) {
    (ALLOC_COUNT.load(Ordering::Relaxed), ALLOC_BYTES.load(Ordering::Relaxed))
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

    fn send(&mut self, _buf: &mut Vec<u8>, _offsets: &[(usize, usize)]) -> Result<(), ()> {
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
                    b.iter(|| {
                        reset_alloc_counters();
                        fixture.run();
                        let (calls, bytes) = snapshot_alloc_counters();
                        std::hint::black_box((calls, bytes));
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
    datapath_forwarder_miss_benchmark
    ,
    datapath_alloc_tracking_benchmark
);
criterion_main!(benches);
