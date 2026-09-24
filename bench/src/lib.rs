use std::hint::black_box;
use std::time::{Duration, Instant};

pub mod token_bench;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchSample {
    pub size: usize,
    pub iterations: usize,
    pub elapsed: Duration,
}

impl BenchSample {
    pub fn bytes_per_second(&self) -> f64 {
        let elapsed_secs = self.elapsed.as_secs_f64();
        if elapsed_secs > 0.0 {
            (self.size.saturating_mul(self.iterations)) as f64 / elapsed_secs
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchReport {
    pub samples: Vec<BenchSample>,
}

/// Allocate a `Vec<u8>` of `size` bytes and fill it with a deterministic pattern.
pub fn alloc_and_fill(size: usize) -> Vec<u8> {
    let mut v = vec![0u8; size];
    fill_pattern(&mut v);
    v
}

static PATTERN_256: [u8; 256] = {
    let mut arr = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        arr[i] = i as u8;
        i += 1;
    }
    arr
};

// Fill buffer with repeating 256-byte sequence using 256-byte chunk copies
fn fill_pattern(buffer: &mut [u8]) {
    let mut chunks = buffer.chunks_exact_mut(256);
    for chunk in chunks.by_ref() {
        chunk.copy_from_slice(&PATTERN_256);
    }
    let remainder = chunks.into_remainder();
    if !remainder.is_empty() {
        remainder.copy_from_slice(&PATTERN_256[..remainder.len()]);
    }
}

pub fn run_bench() -> BenchReport {
    run_bench_with_sizes(&[1024, 8 * 1024, 64 * 1024], 100)
}

pub fn run_bench_with_sizes(sizes: &[usize], iterations: usize) -> BenchReport {
    let mut samples = Vec::with_capacity(sizes.len());
    for &size in sizes {
        let mut buffer = vec![0u8; size];
        let start = Instant::now();
        for _ in 0..iterations {
            fill_pattern(&mut buffer);
            black_box(&buffer);
        }
        samples.push(BenchSample {
            size,
            iterations,
            elapsed: start.elapsed(),
        });
    }
    BenchReport { samples }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_and_fill_uses_expected_pattern() {
        let buf = alloc_and_fill(8);

        assert_eq!(buf, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn run_bench_with_sizes_returns_matching_samples() {
        let report = run_bench_with_sizes(&[4, 8], 2);

        assert_eq!(report.samples.len(), 2);
        assert_eq!(report.samples[0].size, 4);
        assert_eq!(report.samples[0].iterations, 2);
        assert_eq!(report.samples[1].size, 8);
        assert_eq!(report.samples[1].iterations, 2);
    }

    #[test]
    fn bytes_per_second_accounts_for_all_iterations() {
        let sample = BenchSample {
            size: 1_024,
            iterations: 4,
            elapsed: Duration::from_secs(2),
        };

        assert_eq!(sample.bytes_per_second(), 2_048.0);
    }
}
