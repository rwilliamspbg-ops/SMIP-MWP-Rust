//! Price Oracle Reader for DeepBook Predict Markets
//! 
//! Reads oracle prices and feeds them to agent decision logic

use super::OraclePrice;
use std::time::{Duration, SystemTime};

/// Oracle price source types (for provenance tracking)
#[derive(Debug, Clone, PartialEq)]
pub enum OracleSource {
    DeepBookNative,       // Built-in DeepBook oracle
    Chainlink,            // External oracle integration
    LocalSnapshot,        // Local cache with TTL
}

impl std::fmt::Display for OracleSource {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OracleSource::DeepBookNative => write!(f, "DeepBookNative"),
            OracleSource::Chainlink => write!(f, "Chainlink"),
            OracleSource::LocalSnapshot => write!(f, "LocalSnapshot"),
        }
    }
}

/// Oracle reader abstraction
pub struct OracleReader {
    pub source: OracleSource,
    pub last_update: SystemTime,
    pub cache_ttl: Duration,
}

impl Default for OracleReader {
    fn default() -> Self {
        Self {
            source: OracleSource::DeepBookNative,
            last_update: SystemTime::now(),
            cache_ttl: Duration::from_secs(60), // 60s TTL
        }
    }
}

impl OracleReader {
    /// Create new oracle reader with custom TTL
    pub fn new(source: OracleSource, cache_ttl: Duration) -> Self {
        Self {
            source,
            last_update: SystemTime::now(),
            cache_ttl,
        }
    }
    
    /// Read current oracle price for market
    pub async fn read_price(&mut self, market_id: &str) -> Result<OraclePrice, ReaderError> {
        let now = SystemTime::now();
        
        // Check cache first (TTL-based)
        if let Ok(Some(price)) = self.get_cached_price(market_id) {
            self.last_update = now;
            return Ok(OraclePrice {
                market_id: market_id.to_string(),
                current_price: price,
                oracle_source: self.source.clone().to_string(),
                timestamp: now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
            });
        }
        
        // Fetch from DeepBook API (zero-copy via AF_XDP)
        let price = Self::fetch_from_deepbook(market_id).await?;
        self.last_update = now;
        
        Ok(OraclePrice {
            market_id: market_id.to_string(),
            current_price: price,
            oracle_source: self.source.clone().to_string(),
            timestamp: now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
        })
    }
    
    /// Get cached price (if within TTL)
    fn get_cached_price(&self, market_id: &str) -> Result<Option<u128>, ReaderError> {
        // TODO: Implement cache storage/retrieval
        // For now, return None to force API fetch
        Ok(None)
    }
    
    /// Fetch price directly from DeepBook (simulated for MVP)
    async fn fetch_from_deepbook(market_id: &str) -> Result<u128, ReaderError> {
        // In production: zero-copy read from AF_XDP ring buffer
        // For MVP: simulate with deterministic response
        Ok(100_000_000_000u128) // $1.00 in base units
    }
    
    /// Update oracle source (for testing different oracle backends)
    pub fn set_source(&mut self, source: OracleSource) {
        self.source = source;
    }
}

/// Errors for oracle operations
#[derive(Debug, Clone)]
pub enum ReaderError {
    ApiTimeout(String),
    InvalidMarketId(String),
    PriceOutOfRange(u128),
}

impl std::fmt::Display for ReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ReaderError::ApiTimeout(source) => write!(f, "API timeout from {}", source),
            ReaderError::InvalidMarketId(id) => write!(f, "Invalid market ID: {}", id),
            ReaderError::PriceOutOfRange(price) => write!(f, "Price out of range: {}", price),
        }
    }
}

/// Oracle aggregator for multi-source consensus
pub struct OracleAggregator {
    readers: Vec<OracleReader>,
    quorum_threshold: usize,
}

impl OracleAggregator {
    pub fn new(readers: Vec<OracleReader>, quorum: usize) -> Self {
        Self {
            readers,
            quorum_threshold: quorum,
        }
    }
    
    /// Get consensus price from multiple oracles
    pub async fn get_consensus_price(
        &mut self,
        market_id: &str,
    ) -> Result<u128, ReaderError> {
        let mut prices = Vec::new();
        
        for reader in &self.readers {
            let price = reader.read_price(market_id).await?;
            prices.push(price.current_price);
        }
        
        if prices.len() < self.quorum_threshold {
            return Err(ReaderError::ApiTimeout("Insufficient quorum".into()));
        }
        
        // Median consensus (resistant to Byzantine oracle faults)
        let mut sorted = prices.clone();
        sorted.sort();
        let mid = sorted.len() / 2;
        Ok(sorted[mid])
    }
}
