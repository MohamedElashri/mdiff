//! Repository-local, byte-bounded patch reuse. No content is persisted to disk.
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

const BUDGET: usize = 32 * 1024 * 1024;
#[derive(Default)]
pub struct PatchCache(Mutex<Entries>);
#[derive(Default)]
struct Entries {
    values: HashMap<String, (Arc<str>, u64)>,
    order: BTreeMap<u64, String>,
    clock: u64,
    bytes: usize,
    hits: usize,
}
fn cost(key: &str, value: &str) -> usize {
    key.len() * 2 + value.len() + 128
}
impl PatchCache {
    pub fn get(&self, key: &str) -> Option<Arc<str>> {
        let mut entries = self.0.lock().unwrap();
        let (value, previous) = entries.values.get(key)?.clone();
        entries.order.remove(&previous);
        entries.clock += 1;
        let stamp = entries.clock;
        entries.order.insert(stamp, key.to_owned());
        entries.values.get_mut(key).unwrap().1 = stamp;
        entries.hits += 1;
        Some(value)
    }
    pub fn insert(&self, key: String, value: String) {
        let mut entries = self.0.lock().unwrap();
        if let Some((old, stamp)) = entries.values.remove(&key) {
            entries.bytes -= cost(&key, &old);
            entries.order.remove(&stamp);
        }
        let size = cost(&key, &value);
        if size > BUDGET {
            return;
        }
        while entries.bytes + size > BUDGET {
            let Some((_, key)) = entries.order.pop_first() else { break };
            if let Some((value, _)) = entries.values.remove(&key) {
                entries.bytes -= cost(&key, &value);
            }
        }
        entries.clock += 1;
        let stamp = entries.clock;
        entries.bytes += size;
        entries.order.insert(stamp, key.clone());
        entries.values.insert(key, (Arc::from(value), stamp));
    }
    pub fn clear(&self) {
        let mut e = self.0.lock().unwrap();
        e.values.clear();
        e.order.clear();
        e.bytes = 0;
    }
    pub fn stats(&self) -> (usize, usize) {
        let e = self.0.lock().unwrap();
        (e.bytes, e.hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evicts_by_bytes_and_keeps_recent_entries() {
        let cache = PatchCache::default();
        cache.insert("a".into(), "x".repeat(BUDGET / 2 - 130));
        cache.insert("b".into(), "x".repeat(BUDGET / 2 - 130));
        assert!(cache.get("a").is_some());
        cache.insert("c".into(), "x".into());
        assert!(cache.get("b").is_none());
        assert!(cache.get("a").is_some());
        cache.insert("oversized".into(), "x".repeat(BUDGET));
        assert!(cache.get("oversized").is_none());
        assert!(cache.stats().0 <= BUDGET);
    }
}
