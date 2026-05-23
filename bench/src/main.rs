fn main() {
    let report = bench::run_bench();
    println!("Bench runner: {} sizes", report.samples.len());
    for sample in report.samples {
        let avg_ns = sample.elapsed.as_secs_f64() * 1_000_000_000.0 / sample.iterations as f64;
        let mebibytes_per_sec = sample.bytes_per_second() / (1024.0 * 1024.0);
        println!(
            "size={} avg_ns={:.2} throughput_mib_s={:.2}",
            sample.size,
            avg_ns,
            mebibytes_per_sec,
        );
    }

    let token_results = bench::token_bench::run_token_bench_multi(&[(128, 512), (256, 256)], 10);
    println!("Token bench runner: {} configs", token_results.len());
    for result in token_results {
        result.print();
    }
}
