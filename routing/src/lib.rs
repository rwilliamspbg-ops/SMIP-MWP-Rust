// routing crate: route table and lookup (implementation)

use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{SystemTime};

#[derive(Clone, Debug)]
pub struct RouteEntry {
    pub dest_id: [u8;32],
    pub next_hop_id: [u8;32],
    pub metric: i32,
    pub last_seen: SystemTime,
}

#[derive(Debug)]
pub struct Table {
    inner: RwLock<TableInner>,
}

#[derive(Debug)]
struct TableInner {
    entries: HashMap<[u8;32], RouteEntry>,
    sorted_keys: Vec<[u8;32]>,
}

impl Table {
    pub fn new() -> Self {
        Self { inner: RwLock::new(TableInner { entries: HashMap::new(), sorted_keys: Vec::new() }) }
    }

    pub fn update_route(&self, e: RouteEntry) {
        let mut inner = self.inner.write();
        let mut e = e;
        e.last_seen = SystemTime::now();
        inner.entries.insert(e.dest_id, e);
        inner.sorted_keys = inner.entries.keys().cloned().collect();
        inner.sorted_keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    }

    pub fn remove_route(&self, dest: [u8;32]) {
        let mut inner = self.inner.write();
        inner.entries.remove(&dest);
        inner.sorted_keys = inner.entries.keys().cloned().collect();
        inner.sorted_keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    }

    pub fn lookup_next_hop(&self, dst_id: [u8;32], _flow_label: u32) -> Option<[u8;32]> {
        let inner = self.inner.read();
        inner.entries.get(&dst_id).map(|e| e.next_hop_id)
    }

    pub fn predictive_next_hop(&self, src_id: [u8;32], dst_id: [u8;32], flow_label: u32) -> Option<[u8;32]> {
        let inner = self.inner.read();
        if inner.entries.is_empty() {
            return None;
        }
        if let Some(e) = inner.entries.get(&dst_id) {
            return Some(e.next_hop_id);
        }
        let keys = &inner.sorted_keys;
        if keys.is_empty() {
            return None;
        }
        let mut hasher = Sha256::new();
        hasher.update(&src_id);
        hasher.update(&dst_id);
        hasher.update(&flow_label.to_be_bytes());
        let sum = hasher.finalize();
        let idx = u32::from_be_bytes([sum[0], sum[1], sum[2], sum[3]]) as usize % keys.len();
        let chosen = inner.entries.get(&keys[idx]).unwrap();
        Some(chosen.next_hop_id)
    }

    pub fn lookup_or_predict(&self, src_id: [u8;32], dst_id: [u8;32], flow_label: u32) -> Option<[u8;32]> {
        if let Some(nh) = self.lookup_next_hop(dst_id, flow_label) {
            return Some(nh);
        }
        self.predictive_next_hop(src_id, dst_id, flow_label)
    }
}

// Router and policies
#[derive(Clone, Debug)]
pub struct RoutePolicy {
    pub next_hop_id: [u8;32],
    pub queue_id: i32,
    pub priority: i32,
}

#[derive(Debug)]
pub struct Router {
    inner: RwLock<HashMap<u64, RoutePolicy>>,
}

impl Router {
    pub fn new() -> Self {
        let r = Self { inner: RwLock::new(HashMap::new()) };
        r.seed_default_policies();
        r
    }

    fn seed_default_policies(&self) {
        let mut m = self.inner.write();
        m.insert(0, RoutePolicy { next_hop_id: [0u8;32], queue_id: 0, priority: 10 });
    }

    fn compute_flow_key(&self, src_id: [u8;32], dst_id: [u8;32], flow_label: u32) -> u64 {
        let mut key: u64 = 0;
        for i in 0..8 {
            let a = src_id[i*4] as u64;
            let b = dst_id[i*4] as u64;
            key ^= (a << 32) | b;
        }
        key ^ (flow_label as u64)
    }

    pub fn lookup_policy(&self, src_id: [u8;32], dst_id: [u8;32], flow_label: u32) -> Result<RoutePolicy, &'static str> {
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

    pub fn update_policy(&self, src_id: [u8;32], dst_id: [u8;32], flow_label: u32, next_hop_id: [u8;32], queue_id: i32) {
        let key = self.compute_flow_key(src_id, dst_id, flow_label);
        let mut m = self.inner.write();
        m.insert(key, RoutePolicy { next_hop_id, queue_id, priority: 1 });
        println!("SUCCESS: Policy updated for key {:x} -> Queue {}", key, queue_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn update_and_lookup() {
        let t = Table::new();
        let dest = [3u8;32];
        let next = [7u8;32];
        t.update_route(RouteEntry { dest_id: dest, next_hop_id: next, metric: 0, last_seen: SystemTime::now() });
        let got = t.lookup_next_hop(dest, 0).unwrap();
        assert_eq!(got, next);
    }

    #[test]
    fn predictive_choice() {
        let t = Table::new();
        // insert two entries
        t.update_route(RouteEntry { dest_id: [1u8;32], next_hop_id: [9u8;32], metric: 0, last_seen: SystemTime::now() });
        t.update_route(RouteEntry { dest_id: [2u8;32], next_hop_id: [8u8;32], metric: 0, last_seen: SystemTime::now() });
        let src = [4u8;32];
        let dst = [99u8;32];
        let choice = t.predictive_next_hop(src, dst, 7).unwrap();
        // choice should be one of the next hop ids
        assert!(choice == [9u8;32] || choice == [8u8;32]);
    }

    #[test]
    fn remove_route_and_lookup_policy() {
        let t = Table::new();
        let dest = [5u8; 32];
        let nh = [7u8; 32];
        t.update_route(RouteEntry { dest_id: dest, next_hop_id: nh, metric: 1, last_seen: SystemTime::now() });
        assert_eq!(t.lookup_or_predict([1u8; 32], dest, 0).unwrap(), nh);
        t.remove_route(dest);
        assert!(t.lookup_next_hop(dest, 0).is_none());

        let router = Router::new();
        let policy = router.lookup_policy([1u8; 32], [2u8; 32], 7).expect("default policy");
        assert_eq!(policy.queue_id, 0);
        router.update_policy([1u8; 32], [2u8; 32], 7, [9u8; 32], 3);
        let updated = router.lookup_policy([1u8; 32], [2u8; 32], 7).expect("updated policy");
        assert_eq!(updated.queue_id, 3);
        assert_eq!(updated.next_hop_id, [9u8; 32]);
    }
}
