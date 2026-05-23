use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchSample {
    pub size: usize,
    pub iterations: usize,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchReport {
    pub samples: Vec<BenchSample>,
}

/// Allocate a `Vec<u8>` of `size` bytes and fill it with a deterministic pattern.
pub fn alloc_and_fill(size: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(size);
    unsafe { v.set_len(size); }
    for i in 0..size {
        v[i] = (i & 0xFF) as u8;
    }
    v
}

pub fn run_bench() -> BenchReport {
    run_bench_with_sizes(&[1024, 8 * 1024, 64 * 1024], 100)
}

pub fn run_bench_with_sizes(sizes: &[usize], iterations: usize) -> BenchReport {
    let mut samples = Vec::with_capacity(sizes.len());
    for &size in sizes {
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = alloc_and_fill(size);
        }
        samples.push(BenchSample {
            size,
            iterations,
            elapsed: start.elapsed(),
        });
    }
    BenchReport { samples }
}
