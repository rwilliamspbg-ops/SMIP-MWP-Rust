use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use bench::alloc_and_fill;

fn alloc_fill_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_and_fill");
    // use a fixed size that's realistic but quick on CI
    let sizes = [1024usize, 8 * 1024, 64 * 1024];
    for &size in &sizes {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(format!("size_{}", size), &size, |b, &s| {
            b.iter(|| {
                // function returns the buffer — drop it to avoid retention
                let _ = alloc_and_fill(s);
            })
        });
    }
    group.finish();
}

criterion_group!(benches, alloc_fill_benchmark);
criterion_main!(benches);
