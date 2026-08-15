use ahash::AHashMap;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct RouteEntry {
    pub dest_id: [u8; 32],
    pub next_hop_id: [u8; 32],
    pub metric: i32,
    pub last_seen: SystemTime,
    /// MCR-specific: number of alternate channels (default 1 = single-path)
    pub channel_count: u8,
    /// List of alternative next-hop IDs for spraying (may be empty)
    pub alternate_channels: Vec<[u8; 32]>,
    /// MCR epoch for failover decisions (monotonic counter)
    pub mcr_epoch: u64,
}

#[derive(Debug)]
pub struct ChannelStats {
    /// Per-next-hop forwarded packet counters
    pub per_channel_forwarded: AHashMap<[u8; 32], AtomicU64>,
    /// Dropped packets for this destination
    pub packets_dropped: AtomicU64,
    pub last_failure: Option<SystemTime>,
    pub failure_count: u32,
}

impl Default for ChannelStats {
    fn default() -> Self {
        ChannelStats {
            per_channel_forwarded: AHashMap::new(),
            packets_dropped: AtomicU64::new(0),
            last_failure: None,
            failure_count: 0,
        }
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct Table {
    inner: RwLock<TableInner>,
    fast_shards: Vec<RwLock<AHashMap<[u8; 32], RouteEntry>>>,
    /// MCR-specific: per-destination channel stats (read-only hot path)
    mcr_channel_stats: RwLock<AHashMap<[u8; 32], ChannelStats>>,
}

#[derive(Clone, Debug)]
struct TableInner {
    // BTreeMap keeps keys sorted automatically — no manual re-sort needed
    entries: BTreeMap<[u8; 32], RouteEntry>,
    predictive_entries: Vec<RouteEntry>,
}

const HOT_CACHE_SIZE: usize = 256;
const FAST_SHARDS: usize = 16;

static GLOBAL_TABLE_EPOCH: AtomicU64 = AtomicU64::new(1);

struct ThreadCache {
    epochs: [u64; HOT_CACHE_SIZE],
    dest_ids: [[u8; 32]; HOT_CACHE_SIZE],
    next_hops: [[u8; 32]; HOT_CACHE_SIZE],
}

thread_local! {
    static THREAD_CACHE: std::cell::UnsafeCell<ThreadCache> = const { std::cell::UnsafeCell::new(ThreadCache {
        epochs: [0; HOT_CACHE_SIZE],
        dest_ids: [[0; 32]; HOT_CACHE_SIZE],
        next_hops: [[0; 32]; HOT_CACHE_SIZE],
    }) };
}

struct SprayCache {
    epochs: [u64; HOT_CACHE_SIZE],
    dest_ids: [[u8; 32]; HOT_CACHE_SIZE],
    flow_labels: [u32; HOT_CACHE_SIZE],
    next_hops: [[u8; 32]; HOT_CACHE_SIZE],
}

thread_local! {
    static SPRAY_CACHE: std::cell::UnsafeCell<SprayCache> = const { std::cell::UnsafeCell::new(SprayCache {
        epochs: [0; HOT_CACHE_SIZE],
        dest_ids: [[0; 32]; HOT_CACHE_SIZE],
        flow_labels: [0; HOT_CACHE_SIZE],
        next_hops: [[0; 32]; HOT_CACHE_SIZE],
    }) };
}

/// Fast non-cryptographic hash of (src_id, dst_id, flow_label) used for
/// predictive routing. AHasher replaces the previous SipHash-backed default
/// hasher, reducing per-miss cost on the trusted datapath hot path.
/// We use native 64-bit integer pointer casting and XOR folding via `read_unaligned`
/// to avoid generic hashing and slice-length overhead on the hot path entirely.
fn fast_flow_hash(src_id: &[u8; 32], dst_id: &[u8; 32], flow_label: u32) -> u64 {
    let s_ptr = src_id.as_ptr() as *const u64;
    let d_ptr = dst_id.as_ptr() as *const u64;
    let s0 = unsafe { s_ptr.read_unaligned() };
    let s1 = unsafe { s_ptr.add(1).read_unaligned() };
    let s2 = unsafe { s_ptr.add(2).read_unaligned() };
    let s3 = unsafe { s_ptr.add(3).read_unaligned() };
    let d0 = unsafe { d_ptr.read_unaligned() };
    let d1 = unsafe { d_ptr.add(1).read_unaligned() };
    let d2 = unsafe { d_ptr.add(2).read_unaligned() };
    let d3 = unsafe { d_ptr.add(3).read_unaligned() };
    s0 ^ s1 ^ s2 ^ s3 ^ d0 ^ d1 ^ d2 ^ d3 ^ (flow_label as u64)
}

impl Table {
    pub fn new() -> Self {
        let init = TableInner {
            entries: BTreeMap::new(),
            predictive_entries: Vec::new(),
        };
        let mut shards = Vec::with_capacity(FAST_SHARDS);
        for _ in 0..FAST_SHARDS {
            shards.push(RwLock::new(AHashMap::new()));
        }
        Self {
            inner: RwLock::new(init),
            fast_shards: shards,
            mcr_channel_stats: RwLock::new(AHashMap::new()),
        }
    }

    #[inline]
    fn hash_32(dest_id: &[u8; 32]) -> u64 {
        let ptr = dest_id.as_ptr() as *const u64;
        let h0 = unsafe { ptr.read_unaligned() };
        let h1 = unsafe { ptr.add(1).read_unaligned() };
        let h2 = unsafe { ptr.add(2).read_unaligned() };
        let h3 = unsafe { ptr.add(3).read_unaligned() };
        h0 ^ h1 ^ h2 ^ h3
    }

    #[inline]
    fn shard_for_from_hash(h: u64) -> usize {
        (h as usize) & (FAST_SHARDS - 1)
    }

    fn rebuild_predictive_entries(inner: &mut TableInner) {
        inner.predictive_entries = inner.entries.values().cloned().collect();
    }

    pub fn update_route(&self, e: RouteEntry) {
        let mut e = e;
        e.last_seen = SystemTime::now();
        let dest_id = e.dest_id;
        // update fast-path shard first
        let h = Self::hash_32(&dest_id);
        let shard = Self::shard_for_from_hash(h);
        {
            let mut map = self.fast_shards[shard].write();
            map.insert(dest_id, e.clone());
        }

        // update main table under write lock
        {
            let mut inner = self.inner.write();
            inner.entries.insert(dest_id, e);
            Self::rebuild_predictive_entries(&mut inner);
        }
        // ensure channel stats entry exists
        {
            let mut stats = self.mcr_channel_stats.write();
            stats.entry(dest_id).or_default();
        }
        // Invalidate per-thread caches
        GLOBAL_TABLE_EPOCH.fetch_add(1, Ordering::AcqRel);
    }

    #[inline]
    fn simple_cache_index(dest_id: &[u8; 32]) -> usize {
        let ptr = dest_id.as_ptr() as *const u32;
        let val = unsafe { ptr.read_unaligned() };
        // Use Knuth's multiplicative hashing to scramble the index across the cache size
        let hash = val.wrapping_mul(0x9e3779b9);
        (hash as usize) & (HOT_CACHE_SIZE - 1)
    }

    #[inline]
    fn spray_cache_index(dest_id: &[u8; 32], flow_label: u32) -> usize {
        let ptr = dest_id.as_ptr() as *const u32;
        let val = unsafe { ptr.read_unaligned() };
        // Use Knuth's multiplicative hashing to scramble both the dest_id fragment and flow_label across the cache size
        let hash = (val ^ flow_label).wrapping_mul(0x9e3779b9);
        (hash as usize) & (HOT_CACHE_SIZE - 1)
    }

    #[inline]
    #[allow(dead_code)]
    fn cache_index_from_hash(h: u64) -> usize {
        let folded = (h ^ (h >> 32)) as usize;
        folded & (HOT_CACHE_SIZE - 1)
    }

    fn cache_hot_entry_with_idx(idx: usize, cur_epoch: u64, dest_id: [u8; 32], next_hop: [u8; 32]) {
        THREAD_CACHE.with(|c| {
            let cache = unsafe { &mut *c.get() };
            cache.epochs[idx] = cur_epoch;
            cache.dest_ids[idx] = dest_id;
            cache.next_hops[idx] = next_hop;
        });
    }

    /// Increment per-channel forwarded counter for `dest_id` and `next_hop`.
    pub fn inc_channel_forwarded(&self, dest_id: [u8; 32], next_hop: [u8; 32]) {
        let mut stats = self.mcr_channel_stats.write();
        let entry = stats.entry(dest_id).or_default();
        if let Some(counter) = entry.per_channel_forwarded.get(&next_hop) {
            counter.fetch_add(1, Ordering::Relaxed);
        } else {
            entry
                .per_channel_forwarded
                .insert(next_hop, AtomicU64::new(1));
        }
    }

    /// Increment dropped counter for `dest_id`.
    pub fn inc_channel_dropped(&self, dest_id: [u8; 32]) {
        let mut stats = self.mcr_channel_stats.write();
        let entry = stats.entry(dest_id).or_default();
        entry
            .packets_dropped
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Apply a batch of forwarded and dropped counters with a single write lock.
    pub fn apply_mcr_batch_stats(
        &self,
        forwarded: &HashMap<[u8; 32], HashMap<[u8; 32], u64>>,
        dropped: &HashMap<[u8; 32], u64>,
    ) {
        let mut stats = self.mcr_channel_stats.write();

        for (dest, by_next_hop) in forwarded.iter() {
            let entry = stats.entry(*dest).or_default();
            for (next_hop, count) in by_next_hop.iter() {
                let counter = entry
                    .per_channel_forwarded
                    .entry(*next_hop)
                    .or_insert_with(|| AtomicU64::new(0));
                counter.fetch_add(*count, Ordering::Relaxed);
            }
        }

        for (dest, count) in dropped.iter() {
            let entry = stats.entry(*dest).or_default();
            entry.packets_dropped.fetch_add(*count, Ordering::Relaxed);
        }
    }

    /// Collect MCR metrics snapshot as triples (dest_hex, next_hop_hex, count).
    pub fn collect_mcr_metrics(&self) -> Vec<(String, String, u64)> {
        let stats = self.mcr_channel_stats.read();
        let mut out = Vec::new();
        for (dest, ch_stats) in stats.iter() {
            for (nh, counter) in ch_stats.per_channel_forwarded.iter() {
                let count = counter.load(std::sync::atomic::Ordering::Relaxed);
                out.push((hex::encode(dest), hex::encode(nh), count));
            }
            let dropped = ch_stats
                .packets_dropped
                .load(std::sync::atomic::Ordering::Relaxed);
            if dropped > 0 {
                out.push((hex::encode(dest), "dropped".to_string(), dropped));
            }
        }
        out
    }

    pub fn remove_route(&self, dest: [u8; 32]) {
        // remove from fast-path shard first
        let h = Self::hash_32(&dest);
        let shard = Self::shard_for_from_hash(h);
        {
            let mut map = self.fast_shards[shard].write();
            map.remove(&dest);
        }

        // update main table under write lock
        {
            let mut inner = self.inner.write();
            inner.entries.remove(&dest);
            Self::rebuild_predictive_entries(&mut inner);
        }
        {
            let mut stats = self.mcr_channel_stats.write();
            stats.remove(&dest);
        }
        GLOBAL_TABLE_EPOCH.fetch_add(1, Ordering::AcqRel);
    }

    /// Return list of channels for spraying for given destination and flow label.
    /// Each tuple is `(next_hop_id, is_primary)` where primary is first element.
    /// Optimized: holds the read lock and avoids cloning RouteEntry (which contains a heap-allocated Vec).
    /// Fast_shards is an all-inclusive hash index of all route entries in `Table`, making fallback to
    /// main table BTreeMap search redundant on misses.
    pub fn lookup_spray(&self, dst_id: [u8; 32], flow_label: u32) -> Vec<([u8; 32], bool)> {
        let h = Self::hash_32(&dst_id);
        let shard = Self::shard_for_from_hash(h);

        let map = self.fast_shards[shard].read();
        if let Some(e) = map.get(&dst_id) {
            // construct channels vector: primary + alternates
            let mut out = Vec::with_capacity(1 + e.alternate_channels.len());
            out.push((e.next_hop_id, true));
            for ch in &e.alternate_channels {
                out.push((*ch, false));
            }
            // If there are multiple channels, re-order by hash selection so primary reflects flow affinity
            if out.len() > 1 {
                let choices = out.len();
                let idx = (flow_label as usize) % choices;
                out.swap(0, idx);
                // mark primary accordingly
                for (i, (_, is_primary)) in out.iter_mut().enumerate() {
                    *is_primary = i == 0;
                }
            }
            return out;
        }

        Vec::new()
    }

    /// Return the primary next-hop for spray-mode forwarding without
    /// allocating a channel vector.
    /// Optimized: holds read lock and performs flow-affinity spraying over alternate channels
    /// directly on reference to avoid cloning RouteEntry.
    /// Fast_shards is an all-inclusive hash index of all route entries in `Table`, making fallback to
    /// main table BTreeMap search redundant on misses.
    pub fn lookup_spray_primary(&self, dst_id: [u8; 32], flow_label: u32) -> Option<[u8; 32]> {
        let cur_epoch = GLOBAL_TABLE_EPOCH.load(Ordering::Acquire);
        let idx = Self::spray_cache_index(&dst_id, flow_label);

        if let Some(v) = SPRAY_CACHE.with(|c| {
            let cache = unsafe { &*c.get() };
            if cache.epochs[idx] == cur_epoch
                && cache.dest_ids[idx] == dst_id
                && cache.flow_labels[idx] == flow_label
            {
                Some(cache.next_hops[idx])
            } else {
                None
            }
        }) {
            return Some(v);
        }

        let h = Self::hash_32(&dst_id);
        let shard = Self::shard_for_from_hash(h);

        let nh = {
            let map = self.fast_shards[shard].read();
            if let Some(entry) = map.get(&dst_id) {
                if entry.alternate_channels.is_empty() {
                    Some(entry.next_hop_id)
                } else {
                    let choices = 1 + entry.alternate_channels.len();
                    // Since dst_id is identical to itself, fast_flow_hash(&dst_id, &dst_id, flow_label)
                    // mathematically XOR-cancels the 32-byte arrays completely, yielding exactly flow_label.
                    // We use direct flow_label as index to avoid 8 unaligned reads & multiple XOR operations.
                    let idx = (flow_label as usize) % choices;
                    if idx == 0 {
                        Some(entry.next_hop_id)
                    } else {
                        Some(entry.alternate_channels[idx - 1])
                    }
                }
            } else {
                None
            }
        };

        if let Some(v) = nh {
            SPRAY_CACHE.with(|c| {
                let cache = unsafe { &mut *c.get() };
                cache.epochs[idx] = cur_epoch;
                cache.dest_ids[idx] = dst_id;
                cache.flow_labels[idx] = flow_label;
                cache.next_hops[idx] = v;
            });
        }

        nh
    }

    /// Select a single channel by index (round-robin if out of range)
    pub fn lookup_spray_single(
        &self,
        dst_id: [u8; 32],
        flow_label: u32,
        channel_idx: usize,
    ) -> Option<[u8; 32]> {
        let channels = self.lookup_spray(dst_id, flow_label);
        if channels.is_empty() {
            return None;
        }
        let idx = channel_idx % channels.len();
        Some(channels[idx].0)
    }

    pub fn lookup_next_hop(&self, dst_id: [u8; 32], _flow_label: u32) -> Option<[u8; 32]> {
        // Fast per-thread hot-key cache check
        let cur_epoch = GLOBAL_TABLE_EPOCH.load(Ordering::Acquire);
        let idx = Self::simple_cache_index(&dst_id);

        if let Some(v) = THREAD_CACHE.with(|c| {
            let cache = unsafe { &*c.get() };
            if cache.epochs[idx] == cur_epoch && cache.dest_ids[idx] == dst_id {
                Some(cache.next_hops[idx])
            } else {
                None
            }
        }) {
            return Some(v);
        }

        // Fast-path shard lookup (fast_shards is an all-inclusive hash index of all route entries in Table)
        let h = Self::hash_32(&dst_id);
        let shard = Self::shard_for_from_hash(h);
        let nh_opt = {
            let map = self.fast_shards[shard].read();
            map.get(&dst_id).map(|e| e.next_hop_id)
        };
        if let Some(nh) = nh_opt {
            Self::cache_hot_entry_with_idx(idx, cur_epoch, dst_id, nh);
            return Some(nh);
        }

        None
    }

    pub fn predictive_next_hop(
        &self,
        src_id: [u8; 32],
        dst_id: [u8; 32],
        flow_label: u32,
    ) -> Option<[u8; 32]> {
        // 1. Fast per-thread hot-key cache check
        let cur_epoch = GLOBAL_TABLE_EPOCH.load(Ordering::Acquire);
        let idx = Self::simple_cache_index(&dst_id);

        if let Some(v) = THREAD_CACHE.with(|c| {
            let cache = unsafe { &*c.get() };
            if cache.epochs[idx] == cur_epoch && cache.dest_ids[idx] == dst_id {
                Some(cache.next_hops[idx])
            } else {
                None
            }
        }) {
            return Some(v);
        }

        // 2. Fall back to main table
        let inner = self.inner.read();
        if inner.entries.is_empty() {
            return None;
        }

        // For small tables, BTreeMap search is extremely fast.
        // For larger tables, checking fast_shards (AHashMap) is faster to bypass BTreeMap search on misses.
        let mut nh_opt = None;
        if inner.entries.len() <= 8 {
            if let Some(e) = inner.entries.get(&dst_id) {
                nh_opt = Some(e.next_hop_id);
            }
        } else {
            let h = Self::hash_32(&dst_id);
            let shard = Self::shard_for_from_hash(h);
            let map = self.fast_shards[shard].read();
            if let Some(e) = map.get(&dst_id) {
                nh_opt = Some(e.next_hop_id);
            }
        }

        if let Some(nh) = nh_opt {
            Self::cache_hot_entry_with_idx(idx, cur_epoch, dst_id, nh);
            return Some(nh);
        }

        let n = inner.predictive_entries.len();
        let idx = fast_flow_hash(&src_id, &dst_id, flow_label) as usize % n;
        let chosen = inner.predictive_entries.get(idx).unwrap();
        Some(chosen.next_hop_id)
    }

    pub fn lookup_or_predict(
        &self,
        src_id: [u8; 32],
        dst_id: [u8; 32],
        flow_label: u32,
    ) -> Option<[u8; 32]> {
        // 1. Fast per-thread hot-key cache check
        let cur_epoch = GLOBAL_TABLE_EPOCH.load(Ordering::Acquire);
        let idx = Self::simple_cache_index(&dst_id);

        if let Some(v) = THREAD_CACHE.with(|c| {
            let cache = unsafe { &*c.get() };
            if cache.epochs[idx] == cur_epoch && cache.dest_ids[idx] == dst_id {
                Some(cache.next_hops[idx])
            } else {
                None
            }
        }) {
            return Some(v);
        }

        // 2. Fall back to main table
        let inner = self.inner.read();
        if inner.entries.is_empty() {
            return None;
        }

        // For small tables, BTreeMap search is extremely fast.
        // For larger tables, checking fast_shards (AHashMap) is faster to bypass BTreeMap search on misses.
        let mut nh_opt = None;
        if inner.entries.len() <= 8 {
            if let Some(e) = inner.entries.get(&dst_id) {
                nh_opt = Some(e.next_hop_id);
            }
        } else {
            let h = Self::hash_32(&dst_id);
            let shard = Self::shard_for_from_hash(h);
            let map = self.fast_shards[shard].read();
            if let Some(e) = map.get(&dst_id) {
                nh_opt = Some(e.next_hop_id);
            }
        }

        if let Some(nh) = nh_opt {
            Self::cache_hot_entry_with_idx(idx, cur_epoch, dst_id, nh);
            return Some(nh);
        }

        let n = inner.predictive_entries.len();
        let idx = fast_flow_hash(&src_id, &dst_id, flow_label) as usize % n;
        let chosen = inner.predictive_entries.get(idx).unwrap();
        Some(chosen.next_hop_id)
    }
}

#[derive(Clone, Debug)]
pub struct RoutePolicy {
    pub next_hop_id: [u8; 32],
    pub queue_id: i32,
    pub priority: i32,
}

#[derive(Debug)]
pub struct Router {
    inner: RwLock<AHashMap<u64, RoutePolicy>>,
}

impl Router {
    pub fn new() -> Self {
        let r = Self {
            inner: RwLock::new(AHashMap::new()),
        };
        r.seed_default_policies();
        r
    }

    fn seed_default_policies(&self) {
        let mut m = self.inner.write();
        m.insert(
            0,
            RoutePolicy {
                next_hop_id: [0u8; 32],
                queue_id: 0,
                priority: 10,
            },
        );
    }

    fn compute_flow_key(&self, src_id: [u8; 32], dst_id: [u8; 32], flow_label: u32) -> u64 {
        // Since (a << 32) | b operates on non-overlapping bitfields, we can mathematically
        // consolidate the XOR folding of src_id and dst_id bytes individually on the stack.
        // This completely eliminates the loop, index multiplications, and multiple bitwise operations.
        let a = (src_id[0] as u64)
            ^ (src_id[4] as u64)
            ^ (src_id[8] as u64)
            ^ (src_id[12] as u64)
            ^ (src_id[16] as u64)
            ^ (src_id[20] as u64)
            ^ (src_id[24] as u64)
            ^ (src_id[28] as u64);
        let b = (dst_id[0] as u64)
            ^ (dst_id[4] as u64)
            ^ (dst_id[8] as u64)
            ^ (dst_id[12] as u64)
            ^ (dst_id[16] as u64)
            ^ (dst_id[20] as u64)
            ^ (dst_id[24] as u64)
            ^ (dst_id[28] as u64);
        ((a << 32) | b) ^ (flow_label as u64)
    }

    pub fn lookup_policy(
        &self,
        src_id: [u8; 32],
        dst_id: [u8; 32],
        flow_label: u32,
    ) -> Result<RoutePolicy, &'static str> {
        let key = self.compute_flow_key(src_id, dst_id, flow_label);
        let m = self.inner.read();
        if let Some(p) = m.get(&key) {
            return Ok(p.clone());
        }
        if let Some(p) = m.get(&0) {
            return Ok(p.clone());
        }
        Err("no policy available")
    }

    pub fn update_policy(
        &self,
        src_id: [u8; 32],
        dst_id: [u8; 32],
        flow_label: u32,
        next_hop_id: [u8; 32],
        queue_id: i32,
    ) {
        let key = self.compute_flow_key(src_id, dst_id, flow_label);
        let mut m = self.inner.write();
        m.insert(
            key,
            RoutePolicy {
                next_hop_id,
                queue_id,
                priority: 1,
            },
        );
        println!(
            "SUCCESS: Policy updated for key {:x} -> Queue {}",
            key, queue_id
        );
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn update_and_lookup() {
        let t = Table::new();
        let dest = [3u8; 32];
        let next = [7u8; 32];
        t.update_route(RouteEntry {
            dest_id: dest,
            next_hop_id: next,
            metric: 0,
            last_seen: SystemTime::now(),
            channel_count: 1,
            alternate_channels: Vec::new(),
            mcr_epoch: 1,
        });
        let got = t.lookup_next_hop(dest, 0).unwrap();
        assert_eq!(got, next);
    }

    #[test]
    fn predictive_choice() {
        let t = Table::new();
        t.update_route(RouteEntry {
            dest_id: [1u8; 32],
            next_hop_id: [9u8; 32],
            metric: 0,
            last_seen: SystemTime::now(),
            channel_count: 1,
            alternate_channels: Vec::new(),
            mcr_epoch: 1,
        });
        t.update_route(RouteEntry {
            dest_id: [2u8; 32],
            next_hop_id: [8u8; 32],
            metric: 0,
            last_seen: SystemTime::now(),
            channel_count: 1,
            alternate_channels: Vec::new(),
            mcr_epoch: 1,
        });
        let src = [4u8; 32];
        let dst = [99u8; 32];
        let choice = t.predictive_next_hop(src, dst, 7).unwrap();
        assert!(choice == [9u8; 32] || choice == [8u8; 32]);
    }

    #[test]
    fn remove_route_and_lookup_policy() {
        let t = Table::new();
        let dest = [5u8; 32];
        let nh = [7u8; 32];
        t.update_route(RouteEntry {
            dest_id: dest,
            next_hop_id: nh,
            metric: 1,
            last_seen: SystemTime::now(),
            channel_count: 1,
            alternate_channels: Vec::new(),
            mcr_epoch: 1,
        });
        assert_eq!(t.lookup_or_predict([1u8; 32], dest, 0).unwrap(), nh);
        t.remove_route(dest);
        assert!(t.lookup_next_hop(dest, 0).is_none());

        let router = Router::new();
        let policy = router
            .lookup_policy([1u8; 32], [2u8; 32], 7)
            .expect("default policy");
        assert_eq!(policy.queue_id, 0);
        router.update_policy([1u8; 32], [2u8; 32], 7, [9u8; 32], 3);
        let updated = router
            .lookup_policy([1u8; 32], [2u8; 32], 7)
            .expect("updated policy");
        assert_eq!(updated.queue_id, 3);
        assert_eq!(updated.next_hop_id, [9u8; 32]);
    }
}
