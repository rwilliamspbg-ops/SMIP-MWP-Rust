use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use datapath::{Forwarder, XdpSocket};
use routing::{RouteEntry, Table};
use std::time::SystemTime;
use wire::{Header, HEADER_SIZE};

struct MockSocket {
    frames: Vec<Vec<u8>>,
    sent: Vec<Vec<u8>>,
}

impl MockSocket {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            frames: Vec::with_capacity(capacity),
            sent: Vec::new(),
        }
    }

    fn reset(&mut self, frames: &[Vec<u8>]) {
        self.frames.clear();
        self.frames.extend(frames.iter().cloned());
        self.sent.clear();
    }
}

impl XdpSocket for MockSocket {
    fn poll(&mut self, _max: usize) -> Vec<Vec<u8>> {
        self.frames.drain(..).collect()
    }

    fn send(&mut self, pkts: Vec<Vec<u8>>) -> Result<(), ()> {
        self.sent = pkts;
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
    socket: MockSocket,
}

impl DatapathFixture {
    fn new(packet_count: usize, payload_len: usize, miss: bool) -> Self {
        let forwarder = build_forwarder();
        let templates = (0..packet_count)
            .map(|seq| {
                if miss {
                    build_miss_packet(payload_len, seq as u64)
                } else {
                    build_packet(payload_len, seq as u64)
                }
            })
            .collect::<Vec<_>>();
        let socket = MockSocket::with_capacity(packet_count);

        Self {
            forwarder,
            templates,
            socket,
        }
    }

    fn run(&mut self) {
        self.socket.reset(&self.templates);
        self.forwarder.process_batch(&mut self.socket);
    }
}

fn datapath_forwarder_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("datapath_forwarder");
    let packet_counts = [16usize, 64, 256];
    let payload_len = 256usize;

    for &packet_count in &packet_counts {
        group.throughput(Throughput::Elements(packet_count as u64));
        group.bench_with_input(format!("packets_{}", packet_count), &packet_count, |b, &count| {
            let mut fixture = DatapathFixture::new(count, payload_len, false);
            b.iter(|| fixture.run());
        });
    }

    group.finish();
}

fn datapath_forwarder_miss_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("datapath_forwarder_miss");
    let packet_counts = [16usize, 64, 256];
    let payload_len = 256usize;

    for &packet_count in &packet_counts {
        group.throughput(Throughput::Elements(packet_count as u64));
        group.bench_with_input(format!("packets_{}", packet_count), &packet_count, |b, &count| {
            let mut fixture = DatapathFixture::new(count, payload_len, true);
            b.iter(|| fixture.run());
        });
    }

    group.finish();
}

criterion_group!(benches, datapath_forwarder_benchmark, datapath_forwarder_miss_benchmark);
criterion_main!(benches);