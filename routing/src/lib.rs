use ahash::{AHashMap, AHasher};
use parking_lot::RwLock;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
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

const HOT_CACHE_SIZE: usize = 16;
const FAST_SHARDS: usize = 16;

static GLOBAL_TABLE_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Copy, Clone)]
struct CacheEntry {
    epoch: u64,
    dest_id: [u8; 32],
    next_hop: [u8; 32],
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local)]
    static HOT_CACHE: RefCell<[Option<CacheEntry>; HOT_CACHE_SIZE]> = RefCell::new([None; HOT_CACHE_SIZE]);
    #[allow(clippy::missing_const_for_thread_local)]
    static HOT_CACHE_LAST: RefCell<Option<CacheEntry>> = RefCell::new(None);
    #[allow(clippy::missing_const_for_thread_local)]
    static HOT_CACHE_NEXT: RefCell<usize> = RefCell::new(0);
}

fn cache_next_idx() -> usize {
    HOT_CACHE_NEXT.with(|n| {
        let mut v = n.borrow_mut();
        let idx = *v;
        *v = (*v + 1) % HOT_CACHE_SIZE;
        idx
    })
}

/// Fast non-cryptographic hash of (src_id, dst_id, flow_label) used for
/// predictive routing. AHasher replaces the previous SipHash-backed default
/// hasher, reducing per-miss cost on the trusted datapath hot path.
fn fast_flow_hash(src_id: &[u8; 32], dst_id: &[u8; 32], flow_label: u32) -> u64 {
    let mut h = AHasher::default();
    src_id.hash(&mut h);
    dst_id.hash(&mut h);
    flow_label.hash(&mut h);
    h.finish()
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

    fn shard_for(key: &[u8; 32]) -> usize {
        let mut h = AHasher::default();
        key.hash(&mut h);
        (h.finish() as usize) % FAST_SHARDS
    }

    fn rebuild_predictive_entries(inner: &mut TableInner) {
        inner.predictive_entries = inner.entries.values().cloned().collect();
    }

    pub fn update_route(&self, e: RouteEntry) {
        let mut e = e;
        e.last_seen = SystemTime::now();
        // update fast-path shard first
        let shard = Self::shard_for(&e.dest_id);
        {
            let mut map = self.fast_shards[shard].write();
            map.insert(e.dest_id, e.clone());
        }

        // update main table under write lock
        {
            let mut inner = self.inner.write();
            inner.entries.insert(e.dest_id, e.clone());
            Self::rebuild_predictive_entries(&mut inner);
        }
        // ensure channel stats entry exists
        {
            let mut stats = self.mcr_channel_stats.write();
            stats.entry(e.dest_id).or_insert_with(ChannelStats::default);
        }
        // Invalidate per-thread caches
        GLOBAL_TABLE_EPOCH.fetch_add(1, Ordering::AcqRel);
    }

    fn cache_hot_entry(cur_epoch: u64, dest_id: [u8; 32], next_hop: [u8; 32]) {
        let entry = CacheEntry {
            epoch: cur_epoch,
            dest_id,
            next_hop,
        };

        HOT_CACHE_LAST.with(|slot| {
            *slot.borrow_mut() = Some(entry);
        });

        HOT_CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            let idx = cache_next_idx();
            cache[idx] = Some(entry);
        });
    }

    /// Increment per-channel forwarded counter for `dest_id` and `next_hop`.
    pub fn inc_channel_forwarded(&self, dest_id: [u8; 32], next_hop: [u8; 32]) {
        let mut stats = self.mcr_channel_stats.write();
        let entry = stats.entry(dest_id).or_insert_with(ChannelStats::default);
        use std::sync::atomic::Ordering;
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
        let entry = stats.entry(dest_id).or_insert_with(ChannelStats::default);
        entry.packets_dropped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Apply a batch of forwarded and dropped counters with a single write lock.
    pub fn apply_mcr_batch_stats(
        &self,
        forwarded: &HashMap<[u8; 32], HashMap<[u8; 32], u64>>,
        dropped: &HashMap<[u8; 32], u64>,
    ) {
        let mut stats = self.mcr_channel_stats.write();

        for (dest, by_next_hop) in forwarded.iter() {
            let entry = stats.entry(*dest).or_insert_with(ChannelStats::default);
            for (next_hop, count) in by_next_hop.iter() {
                let counter = entry
                    .per_channel_forwarded
                    .entry(*next_hop)
                    .or_insert_with(|| AtomicU64::new(0));
                counter.fetch_add(*count, Ordering::Relaxed);
            }
        }

        for (dest, count) in dropped.iter() {
            let entry = stats.entry(*dest).or_insert_with(ChannelStats::default);
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
            let dropped = ch_stats.packets_dropped.load(std::sync::atomic::Ordering::Relaxed);
            if dropped > 0 {
                out.push((hex::encode(dest), "dropped".to_string(), dropped));
            }
        }
        out
    }

    pub fn remove_route(&self, dest: [u8; 32]) {
        // remove from fast-path shard first
        let shard = Self::shard_for(&dest);
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
    pub fn lookup_spray(&self, dst_id: [u8; 32], flow_label: u32) -> Vec<([u8; 32], bool)> {
        // Try fast-path shard first
        let shard = Self::shard_for(&dst_id);
        if let Some(e) = { let map = self.fast_shards[shard].read(); map.get(&dst_id).cloned() } {
            // construct channels vector: primary + alternates
            let mut out = Vec::with_capacity(1 + e.alternate_channels.len());
            out.push((e.next_hop_id, true));
            for ch in &e.alternate_channels {
                out.push((*ch, false));
            }
            return out;
        }

        // Fall back to main table
        if let Some(e) = { let inner = self.inner.read(); inner.entries.get(&dst_id).cloned() } {
            let mut out = Vec::with_capacity(1 + e.alternate_channels.len());
            out.push((e.next_hop_id, true));
            for ch in &e.alternate_channels {
                out.push((*ch, false));
            }
            // If there are multiple channels, re-order by hash selection so primary reflects flow affinity
            if out.len() > 1 {
                let choices = out.len();
                let idx = (fast_flow_hash(&dst_id, &dst_id, flow_label) as usize) % choices;
                out.swap(0, idx);
                // mark primary accordingly
                for i in 0..out.len() {
                    out[i].1 = i == 0;
                }
            }
            return out;
        }

        Vec::new()
    }

    /// Return the primary next-hop for spray-mode forwarding without
    /// allocating a channel vector.
    pub fn lookup_spray_primary(&self, dst_id: [u8; 32], flow_label: u32) -> Option<[u8; 32]> {
        let shard = Self::shard_for(&dst_id);
        if let Some(next_hop) = { let map = self.fast_shards[shard].read(); map.get(&dst_id).map(|entry| entry.next_hop_id) } {
            return Some(next_hop);
        }

            let next_hop = {
                let inner = self.inner.read();
                if let Some(entry) = inner.entries.get(&dst_id) {
                    if entry.alternate_channels.is_empty() {
                        Some(entry.next_hop_id)
                    } else {
                        let choices = 1 + entry.alternate_channels.len();
                        let idx = (fast_flow_hash(&dst_id, &dst_id, flow_label) as usize) % choices;
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

            if let Some(next_hop) = next_hop {
                return Some(next_hop);
            }

        None
    }

    /// Select a single channel by index (round-robin if out of range)
    pub fn lookup_spray_single(&self, dst_id: [u8; 32], flow_label: u32, channel_idx: usize) -> Option<[u8; 32]> {
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
        if let Some(v) = HOT_CACHE_LAST.with(|slot| {
            slot.borrow().as_ref().and_then(|ent| {
                if ent.epoch == cur_epoch && ent.dest_id == dst_id {
                    Some(ent.next_hop)
                } else {
                    None
                }
            })
        }) {
            return Some(v);
        }
        if let Some(v) = HOT_CACHE.with(|c| {
            let cache = c.borrow();
            for ent in cache.iter().flatten() {
                if ent.epoch == cur_epoch && ent.dest_id == dst_id {
                    return Some(ent.next_hop);
                }
            }
            None
        }) {
            return Some(v);
        }

        // Fast-path shard lookup
        let shard = Self::shard_for(&dst_id);
        {
            let map = self.fast_shards[shard].read();
            if let Some(e) = map.get(&dst_id) {
                return Some(e.next_hop_id);
            }
        }

        // Miss -> try fast-path already checked; fall back to main table under read lock and populate cache using multiple-probe insertion
        let res = {
            let inner = self.inner.read();
            inner.entries.get(&dst_id).map(|e| e.next_hop_id)
        };
        if let Some(nh) = res {
            Self::cache_hot_entry(cur_epoch, dst_id, nh);
        }
        res
    }

    pub fn predictive_next_hop(
        &self,
        src_id: [u8; 32],
        dst_id: [u8; 32],
        flow_label: u32,
    ) -> Option<[u8; 32]> {
        let inner = self.inner.read();
        if inner.entries.is_empty() {
            return None;
        }
        if let Some(e) = inner.entries.get(&dst_id) {
            return Some(e.next_hop_id);
        }
        let n = inner.predictive_entries.len();
        // Fast hash instead of SHA-256 — O(1), ~10 ns vs ~500 ns
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
        let inner = self.inner.read();
        if let Some(e) = inner.entries.get(&dst_id) {
            return Some(e.next_hop_id);
        }
        if inner.entries.is_empty() {
            return None;
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
        let mut key: u64 = 0;
        for i in 0..8 {
            let a = src_id[i * 4] as u64;
            let b = dst_id[i * 4] as u64;
            key ^= (a << 32) | b;
        }
        key ^ (flow_label as u64)
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
