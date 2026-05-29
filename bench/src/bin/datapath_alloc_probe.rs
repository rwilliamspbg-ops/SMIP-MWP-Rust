use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use datapath::socket::{SliceRing, XdpSocket};
use datapath::Forwarder;
use routing::{RouteEntry, Table};
use wire::{Header, HEADER_SIZE};

struct TrackingAlloc;

static CURRENT_ALLOCS: AtomicU64 = AtomicU64::new(0);
static PEAK_ALLOCS: AtomicU64 = AtomicU64::new(0);
static CURRENT_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_BYTES: AtomicU64 = AtomicU64::new(0);

// No per-pointer map: avoid allocations inside the allocator to prevent reentrancy/deadlock.

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let size = layout.size();
            CURRENT_ALLOCS.fetch_add(1, Ordering::Relaxed);
            let cur_bytes = CURRENT_BYTES.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
            update_peak(&PEAK_ALLOCS, CURRENT_ALLOCS.load(Ordering::Relaxed));
            update_peak(&PEAK_BYTES, cur_bytes);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        System.dealloc(ptr, layout);
        CURRENT_ALLOCS.fetch_sub(1, Ordering::Relaxed);
        CURRENT_BYTES.fetch_sub(size as u64, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            let old_size = layout.size();
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
    // no per-pointer map to clear
}

fn snapshot_alloc_counters() -> (u64, u64, u64, u64) {
    (
        CURRENT_ALLOCS.load(Ordering::Relaxed),
        CURRENT_BYTES.load(Ordering::Relaxed),
        PEAK_ALLOCS.load(Ordering::Relaxed),
        PEAK_BYTES.load(Ordering::Relaxed),
    )
}

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

fn build_forwarder() -> Forwarder {
    let routes = Table::new();
    routes.update_route(RouteEntry {
        dest_id: [2u8; 32],
        next_hop_id: [3u8; 32],
        metric: 1,
        last_seen: SystemTime::now(),
        channel_count: 1,
        alternate_channels: Vec::new(),
        mcr_epoch: 1,
    });
    Forwarder::new(routes)
}

fn main() {
    // simple CLI: optional two args: <packet_count> <payload_len>
    let mut args = std::env::args().skip(1);
    let single_packet_count = args.next().and_then(|s| s.parse::<usize>().ok());
    let single_payload_len = args.next().and_then(|s| s.parse::<usize>().ok());

    let packet_counts = if let Some(p) = single_packet_count {
        vec![p]
    } else {
        vec![16usize, 64, 256]
    };
    let payload_sizes = if let Some(p) = single_payload_len {
        vec![p]
    } else {
        vec![64usize, 128, 256, 512, 1024]
    };

    for &packet_count in &packet_counts {
        for &payload_len in &payload_sizes {
            // build fixture
            let mut forwarder = build_forwarder();
            let packet_len = HEADER_SIZE + payload_len;
            let templates = (0..packet_count)
                .map(|seq| build_packet(payload_len, seq as u64))
                .collect::<Vec<_>>();
            let mut socket = SliceSocket::with_capacity(packet_count);
            let mut ring = SliceRing::new(packet_count * 4, packet_len);

            // capture a single run snapshot
            reset_alloc_counters();
            socket.reset(&templates);
            forwarder.process_batch_slices(&mut socket, &mut ring);
            let (calls, bytes, peak_calls, peak_bytes) = snapshot_alloc_counters();
            println!("packets,{},payload,{},current_allocs,{},current_bytes,{},peak_allocs,{},peak_bytes,{}", packet_count, payload_len, calls, bytes, peak_calls, peak_bytes);
        }
    }
}
