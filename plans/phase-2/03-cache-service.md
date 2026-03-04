# Phase 2 / Task 03: Cache Service

**Prerequisites:** Phase 2 Task 01, 02 (can be done in parallel with 02)
**Goal:** Implement the cache service for generator results — compute cache keys, check cache hits, store and retrieve results.

## Spec Reference
`system_plan.md` §7.3 (Caching and Incrementality)

## Cache Key Formula (from spec)

```
generator_cache_key = sha256(
    plugin_name +
    canonical_toml(config) +
    for each input file in sorted order: sha256(file_content) +
    for vendor IP generators only: tool_version + target_part
)
```

## File to Implement
`crates/loom-core/src/generate/cache.rs`

## Types

```rust
use std::path::{Path, PathBuf};
use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};
use crate::error::LoomError;

/// Cache entry stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub cache_key: String,         // hex-encoded sha256
    pub generator_id: String,
    pub created_at: String,        // ISO 8601
    pub produced_files: Vec<PathBuf>,
}

/// The cache service — manages generator result caching.
pub struct CacheService {
    /// Root cache directory: `.build/cache/`
    cache_dir: PathBuf,
}

impl CacheService {
    pub fn new(build_root: &Path) -> Self {
        Self {
            cache_dir: build_root.join("cache"),
        }
    }

    /// Compute a cache key for a generator given its config and input hashes.
    pub fn compute_cache_key(
        &self,
        plugin_name: &str,
        config: Option<&toml::Value>,
        input_file_hashes: &[(String, String)],  // (path, sha256_hex)
        extra_context: &[(&str, &str)],           // e.g., [("tool_version", "2023.2")]
    ) -> String {
        let mut hasher = Sha256::new();

        // Plugin name
        hasher.update(plugin_name.as_bytes());
        hasher.update(b"\0");

        // Config (canonical TOML serialization)
        if let Some(cfg) = config {
            let cfg_str = toml::to_string(cfg).unwrap_or_default();
            hasher.update(cfg_str.as_bytes());
        }
        hasher.update(b"\0");

        // Input file hashes (sorted by path for determinism)
        let mut sorted_inputs = input_file_hashes.to_vec();
        sorted_inputs.sort_by_key(|(path, _)| path.clone());
        for (path, hash) in &sorted_inputs {
            hasher.update(path.as_bytes());
            hasher.update(b":");
            hasher.update(hash.as_bytes());
            hasher.update(b"\0");
        }

        // Extra context (tool version, part, etc.)
        for (key, value) in extra_context {
            hasher.update(key.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
            hasher.update(b"\0");
        }

        hex::encode(hasher.finalize())
    }

    /// Hash a single file's contents. Returns "sha256:<hex>".
    pub fn hash_file(path: &Path) -> Result<String, LoomError> {
        let content = std::fs::read(path)
            .map_err(|e| LoomError::Io { path: path.to_owned(), source: e })?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
    }

    /// Hash all input files for a generator. Returns sorted (path, hash) pairs.
    pub fn hash_input_files(
        &self,
        inputs: &[PathBuf],
    ) -> Result<Vec<(String, String)>, LoomError> {
        let mut hashes = Vec::new();
        for path in inputs {
            if !path.exists() {
                return Err(LoomError::Io {
                    path: path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Generator input file not found",
                    ),
                });
            }
            let hash = Self::hash_file(path)?;
            hashes.push((path.to_string_lossy().into_owned(), hash));
        }
        Ok(hashes)
    }

    /// Check if a cache entry exists for the given key.
    /// Returns the entry if it exists AND all produced files still exist on disk.
    pub fn get(&self, cache_key: &str) -> Result<Option<CacheEntry>, LoomError> {
        let entry_path = self.entry_path(cache_key);
        if !entry_path.exists() { return Ok(None); }

        let content = std::fs::read_to_string(&entry_path)
            .map_err(|e| LoomError::Io { path: entry_path.clone(), source: e })?;

        let entry: CacheEntry = serde_json::from_str(&content)
            .map_err(|e| LoomError::Internal(format!("Cache entry corrupt: {}", e)))?;

        // Verify all produced files still exist
        let all_exist = entry.produced_files.iter().all(|f| f.exists());
        if all_exist { Ok(Some(entry)) } else { Ok(None) }
    }

    /// Store a cache entry.
    pub fn put(&self, entry: &CacheEntry) -> Result<(), LoomError> {
        std::fs::create_dir_all(&self.cache_dir)
            .map_err(|e| LoomError::Io { path: self.cache_dir.clone(), source: e })?;

        let entry_path = self.entry_path(&entry.cache_key);
        let content = serde_json::to_string_pretty(entry)
            .map_err(|e| LoomError::Internal(e.to_string()))?;

        std::fs::write(&entry_path, content)
            .map_err(|e| LoomError::Io { path: entry_path, source: e })
    }

    /// Invalidate all cache entries for generators with `outputs_unknown = true`.
    /// Called at build start if any such generator exists.
    pub fn invalidate_all(&self) -> Result<(), LoomError> {
        if self.cache_dir.exists() {
            std::fs::remove_dir_all(&self.cache_dir)
                .map_err(|e| LoomError::Io { path: self.cache_dir.clone(), source: e })?;
        }
        Ok(())
    }

    fn entry_path(&self, cache_key: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.json", &cache_key[..16]))
    }
}
```

## Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_cache(tmp: &TempDir) -> CacheService {
        CacheService::new(tmp.path())
    }

    #[test]
    fn test_cache_key_deterministic() {
        let tmp = TempDir::new().unwrap();
        let cache = make_cache(&tmp);

        let key1 = cache.compute_cache_key("command", None, &[], &[]);
        let key2 = cache.compute_cache_key("command", None, &[], &[]);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_changes_with_plugin() {
        let tmp = TempDir::new().unwrap();
        let cache = make_cache(&tmp);

        let key1 = cache.compute_cache_key("command", None, &[], &[]);
        let key2 = cache.compute_cache_key("python", None, &[], &[]);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_miss_then_hit() {
        let tmp = TempDir::new().unwrap();
        let cache = make_cache(&tmp);

        let key = cache.compute_cache_key("command", None, &[], &[]);
        assert!(cache.get(&key).unwrap().is_none());

        // Create a file so produced_files check passes
        let produced = tmp.path().join("output.sv");
        std::fs::write(&produced, "// generated").unwrap();

        let entry = CacheEntry {
            cache_key: key.clone(),
            generator_id: "test::gen".to_string(),
            created_at: "2026-03-03T00:00:00Z".to_string(),
            produced_files: vec![produced],
        };
        cache.put(&entry).unwrap();

        let retrieved = cache.get(&key).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().generator_id, "test::gen");
    }

    #[test]
    fn test_cache_miss_when_file_deleted() {
        let tmp = TempDir::new().unwrap();
        let cache = make_cache(&tmp);

        let key = cache.compute_cache_key("command", None, &[], &[]);
        let produced = tmp.path().join("output.sv");
        std::fs::write(&produced, "// generated").unwrap();

        let entry = CacheEntry {
            cache_key: key.clone(),
            generator_id: "test::gen".to_string(),
            created_at: "2026-03-03T00:00:00Z".to_string(),
            produced_files: vec![produced.clone()],
        };
        cache.put(&entry).unwrap();

        // Delete the produced file
        std::fs::remove_file(&produced).unwrap();

        // Now cache should miss
        let retrieved = cache.get(&key).unwrap();
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_hash_file() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("test.yaml");
        std::fs::write(&file_path, "key: value").unwrap();

        let hash1 = CacheService::hash_file(&file_path).unwrap();
        assert!(hash1.starts_with("sha256:"));

        // Same content → same hash
        let hash2 = CacheService::hash_file(&file_path).unwrap();
        assert_eq!(hash1, hash2);

        // Different content → different hash
        std::fs::write(&file_path, "key: other_value").unwrap();
        let hash3 = CacheService::hash_file(&file_path).unwrap();
        assert_ne!(hash1, hash3);
    }
}
```

## Done When

- `cargo test -p loom-core` passes
- Cache keys are deterministic for the same inputs
- Cache keys change when plugin name, config, or input files change
- Cache hits only occur when all produced files exist on disk
- `put()` + `get()` round-trip correctly
