use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use datapath::socket::{SliceRing, XdpSocket};
use datapath::Forwarder;
use routing::{RouteEntry, Table};
use std::time::SystemTime;
use wire::{Header, HEADER_SIZE};

const PACKET_COUNTS: &[usize] = &[16, 64, 256];
const PAYLOAD_LEN: usize = 256;
const PKT_SIZE: usize = HEADER_SIZE + PAYLOAD_LEN;

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

fn make_packet(seq: u64) -> Vec<u8> {
    let mut buf = Header::new_header_buffer(PAYLOAD_LEN);
    let hdr = Header {
        src_id: [1u8; 32],
        dst_id: [2u8; 32],
        flow_label: 0x1,
        seq_num: seq,
        session_id: [0u8; 16],
        flags: 0,
        length: PAYLOAD_LEN as u16,
    };
    hdr.marshal_into(&mut buf).unwrap();
    for (index, byte) in buf[HEADER_SIZE..].iter_mut().enumerate() {
        *byte = (index & 0xFF) as u8;
    }
    buf
}

struct CloneSocket {
    templates: Vec<Vec<u8>>,
    frames: Vec<Vec<u8>>,
}

impl CloneSocket {
    fn new(n: usize) -> Self {
        let templates: Vec<Vec<u8>> = (0..n).map(|i| make_packet(i as u64)).collect();
        let frames = templates.clone();
        Self { templates, frames }
    }

    fn reset(&mut self) {
        self.frames.clear();
        for template in &self.templates {
            self.frames.push(template.clone());
        }
    }
}

impl XdpSocket for CloneSocket {
    fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.frames)
    }

    fn send(&mut self, _buf: &mut Vec<u8>, _offsets: &[(usize, usize)]) -> Result<(), ()> {
        Ok(())
    }
}

struct ZeroCopySocket {
    templates: Vec<Vec<u8>>,
}

impl ZeroCopySocket {
    fn new(n: usize) -> Self {
        Self {
            templates: (0..n).map(|i| make_packet(i as u64)).collect(),
        }
    }
}

impl XdpSocket for ZeroCopySocket {
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

struct DefaultFallbackSocket {
    templates: Vec<Vec<u8>>,
    frames: Vec<Vec<u8>>,
}

impl DefaultFallbackSocket {
    fn new(n: usize) -> Self {
        let templates: Vec<Vec<u8>> = (0..n).map(|i| make_packet(i as u64)).collect();
        let frames = templates.clone();
        Self { templates, frames }
    }

    fn reset(&mut self) {
        self.frames.clear();
        for template in &self.templates {
            self.frames.push(template.clone());
        }
    }
}

impl XdpSocket for DefaultFallbackSocket {
    fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.frames)
    }

    fn send(&mut self, _buf: &mut Vec<u8>, _offsets: &[(usize, usize)]) -> Result<(), ()> {
        Ok(())
    }
}

fn bench_poll_slices_e2e(c: &mut Criterion) {
    let mut group = c.benchmark_group("poll_slices_e2e");

    for &count in PACKET_COUNTS {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("baseline_process_batch", count),
            &count,
            |b, &n| {
                let mut fwd = build_forwarder();
                let mut sock = CloneSocket::new(n);
                b.iter(|| {
                    sock.reset();
                    black_box(fwd.process_batch(&mut sock));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("slice_process_batch_slices", count),
            &count,
            |b, &n| {
                let mut fwd = build_forwarder();
                let mut sock = ZeroCopySocket::new(n);
                let mut ring = SliceRing::new(n * 4, PKT_SIZE);
                b.iter(|| {
                    black_box(fwd.process_batch_slices(&mut sock, &mut ring));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("fallback_poll_slices_default", count),
            &count,
            |b, &n| {
                let mut fwd = build_forwarder();
                let mut sock = DefaultFallbackSocket::new(n);
                let mut ring = SliceRing::new(n * 4, PKT_SIZE);
                b.iter(|| {
                    sock.reset();
                    black_box(fwd.process_batch_slices(&mut sock, &mut ring));
                });
            },
        );
    }

    group.finish();
}

fn bench_poll_cost_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("poll_slices_poll_cost");

    for &count in PACKET_COUNTS {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(
            BenchmarkId::new("clone_poll", count),
            &count,
            |b, &n| {
                let mut sock = CloneSocket::new(n);
                b.iter(|| {
                    sock.reset();
                    let frames = sock.poll(n);
                    black_box(frames);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("zero_copy_poll_slices", count),
            &count,
            |b, &n| {
                let mut sock = ZeroCopySocket::new(n);
                let mut ring = SliceRing::new(n * 4, PKT_SIZE);
                b.iter(|| {
                    black_box(sock.poll_slices(n, &mut ring));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_poll_slices_e2e, bench_poll_cost_only);
criterion_main!(benches);