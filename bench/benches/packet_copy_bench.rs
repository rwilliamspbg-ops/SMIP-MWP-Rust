use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn build_packet(size: usize) -> Vec<u8> {
    let mut packet = vec![0u8; size];
    for (index, byte) in packet.iter_mut().enumerate() {
        *byte = (index & 0xFF) as u8;
    }
    packet
}

#[inline(never)]
fn clone_packet(packet: &[u8]) -> Vec<u8> {
    packet.to_vec()
}

#[inline(never)]
fn copy_into_reused_arena(arena: &mut Vec<u8>, packet: &[u8]) {
    arena.clear();
    arena.extend_from_slice(packet);
    black_box(arena);
}

#[inline(never)]
fn copy_into_resized_arena(arena: &mut Vec<u8>, packet: &[u8]) {
    arena.clear();
    arena.resize(packet.len(), 0);
    unsafe {
        std::ptr::copy_nonoverlapping(packet.as_ptr(), arena.as_mut_ptr(), packet.len());
    }
    black_box(arena);
}

fn packet_copy_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_copy_cost");
    let packet_sizes = [256usize, 1500, 4096, 65536];

    for &size in &packet_sizes {
        let packet = build_packet(size);

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("clone_to_vec", size),
            &packet,
            |b, packet| {
                b.iter(|| {
                    black_box(clone_packet(black_box(packet)));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("extend_from_slice", size),
            &packet,
            |b, packet| {
                let mut arena = Vec::with_capacity(size);
                b.iter(|| {
                    copy_into_reused_arena(&mut arena, black_box(packet));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("copy_nonoverlapping", size),
            &packet,
            |b, packet| {
                let mut arena = Vec::with_capacity(size);
                b.iter(|| {
                    copy_into_resized_arena(&mut arena, black_box(packet));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, packet_copy_benchmark);
criterion_main!(benches);
