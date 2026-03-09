use dashmap::DashMap;
use std::collections::{HashMap, HashSet};

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "me", "show", "with", "and", "or", "for", "of", "to", "my",
    "in", "on", "i", "want", "need", "please", "can", "you", "create", "make", "build", "give",
    "display", "render", "generate", "some", "that", "this", "it", "have", "has",
];

#[derive(Clone)]
struct PromptEntry {
    vector: Vec<(String, f64)>,
    cache_key: String,
}

#[derive(Clone)]
pub struct SemanticCache {
    entries: DashMap<String, Vec<PromptEntry>>,
    threshold: f64,
}

impl SemanticCache {
    pub fn new(threshold: f64) -> Self {
        Self {
            entries: DashMap::new(),
            threshold,
        }
    }

    pub fn find_similar(&self, prompt: &str, catalog_hash: &str) -> Option<String> {
        let entries = self.entries.get(catalog_hash)?;
        let query_vec = Self::vectorize(prompt);
        let mut best_score = 0.0f64;
        let mut best_key = None;

        for entry in entries.iter() {
            let score = Self::cosine_similarity(&query_vec, &entry.vector);
            if score > best_score {
                best_score = score;
                best_key = Some(entry.cache_key.clone());
            }
        }

        if best_score >= self.threshold {
            best_key
        } else {
            None
        }
    }

    pub fn store(&self, prompt: &str, catalog_hash: &str, cache_key: String) {
        let vector = Self::vectorize(prompt);
        let entry = PromptEntry { vector, cache_key };
        self.entries
            .entry(catalog_hash.to_string())
            .or_default()
            .push(entry);
    }

    fn normalize(prompt: &str) -> Vec<String> {
        let stop: HashSet<&str> = STOP_WORDS.iter().copied().collect();
        prompt
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty() && !stop.contains(w))
            .map(|w| w.to_string())
            .collect()
    }

    fn vectorize(prompt: &str) -> Vec<(String, f64)> {
        let words = Self::normalize(prompt);
        let mut counts: HashMap<String, f64> = HashMap::new();
        for w in &words {
            *counts.entry(w.clone()).or_default() += 1.0;
        }
        let len = words.len().max(1) as f64;
        let mut vec: Vec<(String, f64)> = counts
            .into_iter()
            .map(|(k, v)| (k, v / len))
            .collect();
        vec.sort_by(|a, b| a.0.cmp(&b.0));
        vec
    }

    fn cosine_similarity(a: &[(String, f64)], b: &[(String, f64)]) -> f64 {
        let mut dot = 0.0;
        let mut mag_a = 0.0;
        let mut mag_b = 0.0;

        let a_map: HashMap<&str, f64> = a.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let b_map: HashMap<&str, f64> = b.iter().map(|(k, v)| (k.as_str(), *v)).collect();

        let all_keys: HashSet<&str> = a_map.keys().chain(b_map.keys()).copied().collect();
        for key in all_keys {
            let va = a_map.get(key).copied().unwrap_or(0.0);
            let vb = b_map.get(key).copied().unwrap_or(0.0);
            dot += va * vb;
            mag_a += va * va;
            mag_b += vb * vb;
        }

        if mag_a == 0.0 || mag_b == 0.0 {
            return 0.0;
        }
        dot / (mag_a.sqrt() * mag_b.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        let cache = SemanticCache::new(0.85);
        cache.store("sales dashboard", "cat1", "key-1".into());
        assert_eq!(
            cache.find_similar("sales dashboard", "cat1"),
            Some("key-1".into())
        );
    }

    #[test]
    fn similar_with_filler() {
        let cache = SemanticCache::new(0.85);
        cache.store("sales dashboard", "cat1", "key-1".into());
        assert_eq!(
            cache.find_similar("show me a sales dashboard", "cat1"),
            Some("key-1".into())
        );
    }

    #[test]
    fn similar_with_extras() {
        let cache = SemanticCache::new(0.70);
        cache.store("sales dashboard metrics", "cat1", "key-1".into());
        let result = cache.find_similar("Sales Dashboard with metrics", "cat1");
        assert_eq!(result, Some("key-1".into()));
    }

    #[test]
    fn different_prompt_no_match() {
        let cache = SemanticCache::new(0.85);
        cache.store("sales dashboard", "cat1", "key-1".into());
        assert_eq!(cache.find_similar("user profile page", "cat1"), None);
    }

    #[test]
    fn different_catalog_no_match() {
        let cache = SemanticCache::new(0.85);
        cache.store("sales dashboard", "cat1", "key-1".into());
        assert_eq!(cache.find_similar("sales dashboard", "cat2"), None);
    }

    #[test]
    fn empty_cache_no_match() {
        let cache = SemanticCache::new(0.85);
        assert_eq!(cache.find_similar("anything", "cat1"), None);
    }

    #[test]
    fn picks_best_match() {
        let cache = SemanticCache::new(0.5);
        cache.store("sales dashboard revenue", "cat1", "key-revenue".into());
        cache.store("user profile settings", "cat1", "key-profile".into());
        let result = cache.find_similar("sales dashboard", "cat1");
        assert_eq!(result, Some("key-revenue".into()));
    }
}
