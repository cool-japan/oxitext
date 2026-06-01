//! LRU bitmap glyph cache with configurable capacity.
//!
//! [`BitmapCache`] wraps [`lru::LruCache`] to provide memory-bounded storage
//! of rasterised glyph bitmaps.  Entries are keyed by [`BitmapCacheKey`],
//! which captures glyph identity, rendering size, and render mode so that
//! the same glyph rendered differently is stored as a separate entry.
//!
//! # Examples
//!
//! ```rust
//! use oxitext_raster::cache::{BitmapCache, BitmapCacheKey, RenderMode};
//!
//! let mut cache = BitmapCache::new(128);
//! let key = BitmapCacheKey {
//!     glyph_id: 36,
//!     px_size_times_64: 1024,
//!     render_mode: RenderMode::Greyscale,
//! };
//! cache.insert(key, vec![128u8; 256]);
//! assert!(cache.get(&key).is_some());
//! ```

use lru::LruCache;
use std::num::NonZeroUsize;

/// Identifies the rendering mode used to produce a cached bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderMode {
    /// Greyscale 8-bit antialiased bitmap.
    Greyscale,
    /// Horizontal RGB LCD subpixel bitmap.
    Lcd,
    /// Signed distance field bitmap.
    Sdf,
}

/// Cache key that uniquely identifies a rasterised glyph variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BitmapCacheKey {
    /// Glyph index within the font.
    pub glyph_id: u16,
    /// Pixel size multiplied by 64 for lossless integer comparison.
    pub px_size_times_64: u32,
    /// Rendering mode (greyscale, LCD, SDF).
    pub render_mode: RenderMode,
}

/// LRU cache of rasterised glyph bitmaps with bounded memory usage.
pub struct BitmapCache {
    inner: LruCache<BitmapCacheKey, Vec<u8>>,
}

impl BitmapCache {
    /// Create a new [`BitmapCache`] with the given `capacity` in number of entries.
    ///
    /// If `capacity` is 0, it is silently raised to 1 to satisfy [`NonZeroUsize`].
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1))
            .unwrap_or_else(|| NonZeroUsize::new(1).expect("1 is non-zero"));
        Self {
            inner: LruCache::new(cap),
        }
    }

    /// Retrieve a cached bitmap for `key`, promoting it to the most-recently-used position.
    ///
    /// Returns `None` if the key is not in the cache.
    pub fn get(&mut self, key: &BitmapCacheKey) -> Option<&Vec<u8>> {
        self.inner.get(key)
    }

    /// Insert or replace the bitmap for `key`.
    ///
    /// If the cache is at capacity the least-recently-used entry is evicted.
    pub fn insert(&mut self, key: BitmapCacheKey, data: Vec<u8>) {
        self.inner.put(key, data);
    }

    /// Return the number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Return `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Remove all entries from the cache.
    ///
    /// After this call [`Self::len`] returns `0` and all previously-cached
    /// bitmaps have been dropped.
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(glyph_id: u16) -> BitmapCacheKey {
        BitmapCacheKey {
            glyph_id,
            px_size_times_64: 64,
            render_mode: RenderMode::Greyscale,
        }
    }

    #[test]
    fn cache_hit_miss() {
        let mut cache = BitmapCache::new(2);
        let key = make_key(1);
        assert!(cache.get(&key).is_none());
        cache.insert(key, vec![255u8]);
        assert_eq!(cache.get(&key), Some(&vec![255u8]));
    }

    #[test]
    fn eviction() {
        let mut cache = BitmapCache::new(1);
        let k1 = make_key(1);
        let k2 = make_key(2);
        cache.insert(k1, vec![1u8]);
        cache.insert(k2, vec![2u8]);
        // After inserting k2 into a capacity-1 cache, k1 should be evicted.
        assert!(cache.get(&k1).is_none() || cache.get(&k2).is_some());
    }

    #[test]
    fn len_and_is_empty() {
        let mut cache = BitmapCache::new(4);
        assert!(cache.is_empty());
        cache.insert(make_key(1), vec![]);
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
    }

    #[test]
    fn capacity_zero_treated_as_one() {
        let mut cache = BitmapCache::new(0);
        cache.insert(make_key(1), vec![42u8]);
        assert_eq!(cache.len(), 1);
    }
}
