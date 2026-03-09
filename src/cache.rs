use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

use crate::types::UISpec;

#[derive(Clone)]
pub struct SpecCache {
    entries: DashMap<String, CacheEntry>,
    default_ttl: Duration,
}

#[derive(Clone)]
struct CacheEntry {
    spec: UISpec,
    inserted_at: Instant,
    ttl: Duration,
}

impl SpecCache {
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            entries: DashMap::new(),
            default_ttl,
        }
    }

    pub fn get(&self, key: &str) -> Option<UISpec> {
        let entry = self.entries.get(key)?;
        if entry.inserted_at.elapsed() > entry.ttl {
            drop(entry);
            self.entries.remove(key);
            return None;
        }
        Some(entry.spec.clone())
    }

    pub fn set(&self, key: String, spec: UISpec) {
        self.entries.insert(
            key,
            CacheEntry {
                spec,
                inserted_at: Instant::now(),
                ttl: self.default_ttl,
            },
        );
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn cache_key(prompt: &str, catalog_json: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prompt.as_bytes());
        hasher.update(b"|");
        hasher.update(catalog_json.as_bytes());
        let result = hasher.finalize();
        format!("spec:{}", hex::encode(&result[..16]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn dummy_spec() -> UISpec {
        let mut elements = HashMap::new();
        elements.insert(
            "card-1".into(),
            crate::types::UIElement {
                element_type: "Card".into(),
                props: serde_json::json!({"title": "Test"}),
                children: vec![],
            },
        );
        UISpec {
            root: "card-1".into(),
            elements,
        }
    }

    #[test]
    fn cache_hit_and_miss() {
        let cache = SpecCache::new(Duration::from_secs(60));
        let key = SpecCache::cache_key("hello", "{}");

        assert!(cache.get(&key).is_none());

        cache.set(key.clone(), dummy_spec());
        let hit = cache.get(&key);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().root, "card-1");
    }

    #[test]
    fn cache_key_deterministic() {
        let k1 = SpecCache::cache_key("prompt", "catalog");
        let k2 = SpecCache::cache_key("prompt", "catalog");
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_different_inputs() {
        let k1 = SpecCache::cache_key("prompt-a", "catalog");
        let k2 = SpecCache::cache_key("prompt-b", "catalog");
        assert_ne!(k1, k2);
    }

    #[test]
    fn cache_ttl_expiry() {
        let cache = SpecCache::new(Duration::from_millis(1));
        let key = "test-key".to_string();
        cache.set(key.clone(), dummy_spec());
        std::thread::sleep(Duration::from_millis(5));
        assert!(cache.get(&key).is_none());
    }
}
