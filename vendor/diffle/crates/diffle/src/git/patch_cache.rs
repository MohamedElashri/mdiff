//! Repository-local, byte-bounded patch reuse. No content is persisted to disk.
use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::sync::{Arc, Mutex, OnceLock};

use sha1::{Digest, Sha1};

const BUDGET: usize = 32 * 1024 * 1024;

/// Sharing a patch also shares its digest. A page's digest always describes the bytes it serves,
/// even if a concurrent lookup replaces the entry.
pub(crate) struct PatchText {
    text: String,
    digest: OnceLock<String>,
}

impl PatchText {
    pub fn new(text: String) -> Self {
        Self { text, digest: OnceLock::new() }
    }

    pub fn digest(&self) -> &str {
        self.digest.get_or_init(|| hex::encode(Sha1::digest(self.text.as_bytes())))
    }
}

impl Deref for PatchText {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

#[derive(Default)]
pub struct PatchCache(Mutex<Entries>);
#[derive(Default)]
struct Entries {
    values: HashMap<Arc<str>, (Arc<PatchText>, u64)>,
    order: BTreeMap<u64, Arc<str>>,
    clock: u64,
    bytes: usize,
    hits: usize,
}
fn cost(key: &str, value: &PatchText) -> usize {
    key.len() + value.text.capacity() + 192
}
impl PatchCache {
    pub fn get(&self, key: &str) -> Option<Arc<PatchText>> {
        let mut entries = self.0.lock().unwrap();
        let (value, previous) = entries.values.get(key)?.clone();
        let key = entries.order.remove(&previous).expect("cache entry has a timestamp");
        entries.clock += 1;
        let stamp = entries.clock;
        entries.values.get_mut(key.as_ref()).unwrap().1 = stamp;
        entries.order.insert(stamp, key);
        entries.hits += 1;
        Some(value)
    }
    pub fn insert(&self, key: String, value: String) {
        self.insert_shared(key, Arc::new(PatchText::new(value)));
    }
    pub fn insert_shared(&self, key: String, value: Arc<PatchText>) {
        let size = cost(&key, &value);
        if size > BUDGET {
            return;
        }
        let mut entries = self.0.lock().unwrap();
        if let Some((old, stamp)) = entries.values.remove(key.as_str()) {
            entries.bytes -= cost(&key, &old);
            entries.order.remove(&stamp);
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
        let key: Arc<str> = key.into();
        entries.order.insert(stamp, key.clone());
        entries.values.insert(key, (value, stamp));
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
        cache.insert("a".into(), "x".repeat(BUDGET / 2 - 193));
        cache.insert("b".into(), "x".repeat(BUDGET / 2 - 193));
        assert!(cache.get("a").is_some());
        cache.insert("c".into(), "x".into());
        assert!(cache.get("b").is_none());
        assert!(cache.get("a").is_some());
        cache.insert("oversized".into(), "x".repeat(BUDGET));
        assert!(cache.get("oversized").is_none());
        assert!(cache.stats().0 <= BUDGET);
    }
}
