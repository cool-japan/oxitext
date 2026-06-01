//! Bounded LRU shape cache for [`crate::SwashShaper`].
//!
//! [`ShapeCache`] wraps an [`lru::LruCache`] in an `RwLock` so it can be
//! shared across threads.  Cache keys ([`ShapeKey`]) identify a unique
//! shaping request by font identity, text content, and a hash of any
//! variation-axis settings in use.
//!
//! ## Font identity
//!
//! The `font_id` field is the pointer address of the `Arc<[u8]>` holding
//! the font bytes.  This is the same keying strategy used throughout the
//! raster pipeline.  Callers **must** hold the same `Arc` alive across
//! calls for cache hits to occur; a newly constructed `Arc` from the same
//! bytes will have a different pointer and thus be treated as a cache miss.
//!
//! ## Variation axes
//!
//! `axis_values_hash` reserves a slot for future variation-axis support.
//! Set it to `0` for non-variable fonts (the common case in Slice 5a).

use lru::LruCache;
use oxitext_core::ShapedRun;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};

/// Stable identifier for a font resource, derived from `Arc<[u8]>` pointer.
pub type FontId = u64;

/// Cache key for a single shaping request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeKey {
    /// Pointer identity of the `Arc<[u8]>` font bytes.
    pub font_id: FontId,
    /// The exact UTF-8 text being shaped.
    pub text: String,
    /// Hash of any OpenType variation axis settings in use.  Use `0` for
    /// non-variable fonts.
    pub axis_values_hash: u64,
}

impl ShapeKey {
    /// Construct a key from a font `Arc`, text, and an optional axis hash.
    ///
    /// Passes `0` for `axis_values_hash` when called on non-variable fonts.
    pub fn new(font_data: &Arc<[u8]>, text: &str, axis_values_hash: u64) -> Self {
        Self {
            font_id: Arc::as_ptr(font_data) as *const u8 as u64,
            text: text.to_owned(),
            axis_values_hash,
        }
    }
}

/// Thread-safe bounded LRU cache for [`ShapedRun`]s.
///
/// Wraps [`lru::LruCache`] in an [`RwLock`].  Cache misses trigger a write
/// lock, cache hits also require a write lock because LRU must update the
/// recency order on every `get`.
pub struct ShapeCache {
    inner: RwLock<LruCache<ShapeKey, Arc<ShapedRun>>>,
}

impl ShapeCache {
    /// Creates a new cache with the given capacity.
    ///
    /// If `capacity` is 0, falls back to a minimum capacity of 1 so the cache
    /// is always usable without panicking.
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN);
        ShapeCache {
            inner: RwLock::new(LruCache::new(cap)),
        }
    }

    /// Look up a cached [`ShapedRun`] by key.
    ///
    /// Returns `Some(Arc<ShapedRun>)` on a cache hit and updates the LRU order.
    /// Returns `None` on a miss or if the lock is poisoned.
    pub fn get(&self, key: &ShapeKey) -> Option<Arc<ShapedRun>> {
        self.inner.write().ok()?.get(key).cloned()
    }

    /// Insert a [`ShapedRun`] into the cache.
    ///
    /// Evicts the least-recently-used entry if the cache is at capacity.
    /// Silently no-ops if the lock is poisoned.
    pub fn insert(&self, key: ShapeKey, run: Arc<ShapedRun>) {
        if let Ok(mut cache) = self.inner.write() {
            cache.put(key, run);
        }
    }

    /// Returns the number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.inner.read().ok().map_or(0, |g| g.len())
    }

    /// Returns `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for ShapeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.len();
        f.debug_struct("ShapeCache").field("len", &len).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxitext_core::{ShapedGlyph, ShapedRun};

    fn dummy_run(font_data: Arc<[u8]>) -> Arc<ShapedRun> {
        Arc::new(ShapedRun {
            glyphs: smallvec::smallvec![ShapedGlyph {
                gid: 1,
                x_advance: 10.0,
                ..Default::default()
            }],
            font_data,
        })
    }

    #[test]
    fn shape_cache_miss_then_hit() {
        let cache = ShapeCache::new(16);
        let font: Arc<[u8]> = Arc::from(vec![0u8; 4]);
        let key = ShapeKey::new(&font, "hello", 0);

        assert!(cache.get(&key).is_none(), "expected miss on empty cache");
        assert_eq!(cache.len(), 0);

        let run = dummy_run(Arc::clone(&font));
        cache.insert(key.clone(), Arc::clone(&run));

        let hit = cache.get(&key).expect("expected hit after insert");
        assert_eq!(hit.glyphs[0].gid, 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn shape_cache_eviction_at_capacity_one() {
        let cache = ShapeCache::new(1);
        let font: Arc<[u8]> = Arc::from(vec![0u8; 4]);

        let key_a = ShapeKey::new(&font, "aaa", 0);
        let key_b = ShapeKey::new(&font, "bbb", 0);

        let run_a = dummy_run(Arc::clone(&font));
        let run_b = dummy_run(Arc::clone(&font));

        cache.insert(key_a.clone(), run_a);
        assert_eq!(cache.len(), 1);

        cache.insert(key_b.clone(), run_b);
        assert_eq!(
            cache.len(),
            1,
            "capacity 1 — still one entry after second insert"
        );

        // key_a must have been evicted.
        assert!(cache.get(&key_a).is_none(), "key_a should be evicted");
        assert!(cache.get(&key_b).is_some(), "key_b should be present");
    }

    #[test]
    fn shape_cache_zero_capacity_fallback() {
        // capacity 0 falls back to 1; should not panic.
        let cache = ShapeCache::new(0);
        let font: Arc<[u8]> = Arc::from(vec![0u8; 4]);
        let key = ShapeKey::new(&font, "x", 0);
        let run = dummy_run(Arc::clone(&font));
        cache.insert(key.clone(), run);
        assert!(cache.get(&key).is_some());
    }

    #[test]
    fn shape_key_identity_uses_arc_pointer() {
        // Two Arcs with the same bytes but different allocations must differ.
        let bytes = vec![1u8, 2u8, 3u8];
        let arc1: Arc<[u8]> = Arc::from(bytes.clone());
        let arc2: Arc<[u8]> = Arc::from(bytes);
        let k1 = ShapeKey::new(&arc1, "hi", 0);
        let k2 = ShapeKey::new(&arc2, "hi", 0);
        assert_ne!(
            k1, k2,
            "different Arc allocations must produce different keys"
        );
    }
}
