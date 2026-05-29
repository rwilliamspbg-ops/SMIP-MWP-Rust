use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use routing::{RouteEntry, Table};
use std::time::SystemTime;

const ROUTE_COUNTS: &[usize] = &[1, 2, 3, 4, 5, 6, 7, 8, 16];
const LOOKUPS_PER_ITERATION: usize = 256;

fn build_table(route_count: usize) -> Table {
    let table = Table::new();

    for index in 0..route_count {
        table.update_route(RouteEntry {
            dest_id: [index as u8; 32],
            next_hop_id: [(index as u8).wrapping_add(1); 32],
            metric: index as i32,
            last_seen: SystemTime::now(),
            channel_count: 1,
            alternate_channels: Vec::new(),
            mcr_epoch: 1,
        });
    }

    table
}

#[inline(never)]
fn miss_lookup(
    table: &Table,
    src_id: [u8; 32],
    dst_id: [u8; 32],
    flow_label: u32,
) -> Option<[u8; 32]> {
    table.lookup_or_predict(black_box(src_id), black_box(dst_id), black_box(flow_label))
}

fn routing_miss_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("routing_miss_path");

    for &route_count in ROUTE_COUNTS {
        let table = build_table(route_count);
        let src_id = [0xAA; 32];
        let dst_id = [0xFF; 32];
        let flow_label = 0x1;

        group.throughput(Throughput::Elements(LOOKUPS_PER_ITERATION as u64));
        group.bench_with_input(
            BenchmarkId::new("lookup_or_predict_miss", route_count),
            &route_count,
            |b, _| {
                b.iter(|| {
                    for _ in 0..LOOKUPS_PER_ITERATION {
                        black_box(miss_lookup(&table, src_id, dst_id, flow_label));
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, routing_miss_benchmark);
criterion_main!(benches);
