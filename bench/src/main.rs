fn main() {
    let report = bench::run_bench();
    println!("Bench runner: {} sizes", report.samples.len());
    for sample in report.samples {
        let avg_us = sample.elapsed.as_micros() as f64 / sample.iterations as f64;
        let bytes_per_sec = (sample.size as f64) / (avg_us / 1_000_000.0);
        println!("size={} avg_us={:.2} bytes/sec={:.2}", sample.size, avg_us, bytes_per_sec);
    }
}
