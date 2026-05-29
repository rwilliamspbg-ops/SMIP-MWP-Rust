use criterion::{criterion_group, criterion_main, Criterion};
use std::env;
use std::process::Command;
use std::hint::black_box;

fn line_rate_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("line_rate");

    // If no interface is provided, skip heavy real-NIC runs and provide a no-op bench
    let iface = match env::var("MOHAWK_IFACE") {
        Ok(v) => v,
        Err(_) => {
            group.bench_function("skip_no_iface", |b| b.iter(|| black_box(())));
            group.finish();
            return;
        }
    };

    // Warning: This command runs hardware smoke test and requires DUT privileges and
    // a running traffic generator to provide line-rate traffic. It's intentionally
    // conservative and performs one invocation per benchmark iteration.
    group.bench_function("line_rate_real_smoke", |b| {
        b.iter(|| {
            let status = Command::new("bash")
                .arg("-c")
                .arg(format!(
                    "MOHAWK_IFACE={} MOHAWK_QUEUE_ID=0 ./tools/benchmark/real_smoke.sh",
                    iface
                ))
                .status();
            let _ = black_box(status);
        })
    });

    group.finish();
}

criterion_group!(benches, line_rate_benchmark);
criterion_main!(benches);
