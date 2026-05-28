use std::hint::black_box;
use std::time::{Duration, Instant};

/// Benchmark result for token generation
#[derive(Debug, Clone)]
pub struct TokenBenchResult {
    pub tokens_generated: usize,
    pub elapsed_ms: Duration,
    pub tokens_per_second: f64,
}

impl TokenBenchResult {
    pub fn new(tokens: usize, elapsed: Duration) -> Self {
        let elapsed_secs = elapsed.as_secs_f64();
        let tps = if elapsed_secs > 0.0 {
            (tokens as f64 / elapsed_secs).round()
        } else {
            0.0
        };

        Self {
            tokens_generated: tokens,
            elapsed_ms: elapsed,
            tokens_per_second: tps,
        }
    }

    pub fn print(&self) {
        println!("Tokens Generated: {}", self.tokens_generated);
        println!(
            "Elapsed Time: {:.3} us",
            self.elapsed_ms.as_secs_f64() * 1_000_000.0
        );
        println!("Tokens Per Second: {:.2}", self.tokens_per_second);
    }
}

/// Run a token generation benchmark with multiple iterations and sizes
pub fn run_token_bench(
    prompt_tokens: usize,
    completion_tokens: usize,
    iterations: usize,
) -> TokenBenchResult {
    let tokens_per_iteration = prompt_tokens.saturating_add(completion_tokens);
    let start = Instant::now();

    let mut checksum = 0usize;
    for round in 0..iterations {
        for token_index in 0..tokens_per_iteration {
            checksum = checksum.wrapping_add((token_index ^ round).wrapping_add(1));
        }
    }
    black_box(checksum);

    let elapsed = start.elapsed();

    TokenBenchResult::new(tokens_per_iteration.saturating_mul(iterations), elapsed)
}

/// Run benchmark with multiple prompt/completion size combinations
pub fn run_token_bench_multi(
    configs: &[(usize, usize)], // [(prompt_tokens, completion_tokens)]
    iterations: usize,
) -> Vec<TokenBenchResult> {
    let mut results = Vec::with_capacity(configs.len());

    for &(prompt_tokens, completion_tokens) in configs {
        let result = run_token_bench(prompt_tokens, completion_tokens, iterations);
        results.push(result);
    }

    results
}

/// Benchmark configuration options
#[derive(Debug, Clone)]
pub struct TokenBenchConfig {
    pub prompt_size: usize,
    pub completion_size: usize,
    pub iterations: usize,
    pub warmup_iterations: usize,
}

impl Default for TokenBenchConfig {
    fn default() -> Self {
        Self {
            prompt_size: 128,
            completion_size: 512,
            iterations: 10,
            warmup_iterations: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_reports_token_rate() {
        let result = TokenBenchResult::new(2_000, Duration::from_secs(1));

        assert_eq!(result.tokens_generated, 2_000);
        assert_eq!(result.tokens_per_second, 2_000.0);
    }

    #[test]
    fn multi_bench_tracks_each_configuration() {
        let results = run_token_bench_multi(&[(4, 6), (10, 0)], 3);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tokens_generated, 30);
        assert_eq!(results[1].tokens_generated, 30);
    }

    #[test]
    fn config_defaults_match_expected_sizes() {
        let config = TokenBenchConfig::default();

        assert_eq!(config.prompt_size, 128);
        assert_eq!(config.completion_size, 512);
        assert_eq!(config.iterations, 10);
        assert_eq!(config.warmup_iterations, 3);
    }
}
