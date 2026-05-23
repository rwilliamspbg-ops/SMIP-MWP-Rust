#[inline(always)]
fn fill_pattern_scalar_chunked(buffer: &mut [u8]) {
    for (index, byte) in buffer.iter_mut().enumerate() {
        *byte = (index & 0xFF) as u8;
    }
}
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
    let mut v = Vec::with_capacity(size);
    unsafe { v.set_len(size); }
    fill_pattern(&mut v);
    v
}

fn fill_pattern(buffer: &mut [u8]) {
    for (index, byte) in buffer.iter_mut().enumerate() {
        *byte = (index & 0xFF) as u8;
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
