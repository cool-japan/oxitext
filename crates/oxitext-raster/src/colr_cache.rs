//! Thread-local caches for rasterized COLR colour glyphs.
//!
//! Painting a COLR glyph is not a lookup — it walks the glyph's whole paint
//! graph, flattens every layer outline, rasterizes an anti-aliased coverage
//! mask per layer and composites the layers in premultiplied `f32`.  For a real
//! emoji that costs **37–159 µs per glyph in release** and **0.42–1.97 ms in
//! debug** (measured on the vendored COLRv1 fixtures at 64 px), and it is paid
//! again on every single call because the entry points in [`crate::color`] are
//! pure functions of `(font bytes, glyph id, size, palette)`.
//!
//! A caption renderer draws the same emoji at the same size on every frame, so
//! that work is almost entirely redundant.  This module memoizes it in the same
//! shape as [`crate::tl_cache`]: a `thread_local!` [`lru::LruCache`] holding
//! [`Arc`] handles, so a hit is a refcount bump rather than a re-render and
//! threads never contend on a lock.
//!
//! Two caches are kept because the crate has two COLR result shapes:
//!
//! * [`ColorGlyphImage`] — [`crate::color::render_colr_glyph_sized_cached`],
//!   keyed by `(font, glyph id, em size, palette)`.
//! * [`ColorGlyphBitmap`] — [`crate::color::render_colr_cached`], keyed by
//!   `(font, glyph id, width, height, palette)`.
//!
//! # Why the cached entry points take `&Arc<[u8]>`
//!
//! The uncached entry points take `&[u8]`, and there is no sound *and* cheap
//! way to key a cache on a bare slice:
//!
//! * A **content hash of a sample** (the first 64 bytes, as
//!   [`crate::tl_cache`] uses) is O(1) but not an identity.  Two fonts that
//!   agree on the sampled bytes collide, and unlike the fontdue cache — where a
//!   collision merely parses a font that is still a real font — a collision
//!   here hands back a completely unrelated *picture*.  Programmatically
//!   generated variants of one font (a re-packed sfnt with an edited `COLR`
//!   table, say) collide on any bounded sample by construction.
//! * A **content hash of the whole font** is an identity but costs
//!   milliseconds for a 4.6 MB emoji font — more than the render it would be
//!   saving.
//! * A **raw pointer** is O(1) but not stable: once the caller's buffer is
//!   freed the allocator happily hands the same address to a different font,
//!   and a loop that builds and drops same-sized font variants hits that case
//!   routinely.
//!
//! Taking the caller's [`Arc<[u8]>`] and *retaining it inside the cache entry*
//! makes the pointer sound: while an entry is resident, its font allocation
//! cannot be freed, so no other font can occupy that address.  Dropping the
//! entry drops the handle.  This is also the identity
//! [`crate::FontdueRasterizer`] already keys its font cache on, so callers that
//! hold glyph runs (`oxitext_core::PositionedGlyph::font_data` is an
//! `Arc<[u8]>`) need no new plumbing.
//!
//! # Bounds
//!
//! A colour glyph is a bitmap, so an entry-count bound alone is not enough: one
//! 4096 px entry is 64 MiB. Each cache is therefore bounded twice — by entry
//! count (256) and by total pixel bytes (8 MiB) — and a single result larger
//! than 2 MiB is returned without being stored, so one oversized glyph cannot
//! evict a whole working set.

use std::cell::RefCell;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Arc;

use crate::color::{ColorGlyphBitmap, ColorGlyphImage};

/// Maximum number of rendered colour glyphs kept per cache, per thread.
const COLR_CACHE_ENTRIES: usize = 256;

/// [`COLR_CACHE_ENTRIES`] as the `NonZeroUsize` [`lru::LruCache`] wants,
/// resolved at compile time so construction has no fallible path.
const COLR_CACHE_CAP: NonZeroUsize = match NonZeroUsize::new(COLR_CACHE_ENTRIES) {
    Some(cap) => cap,
    // Unreachable: the constant above is a non-zero literal.  A `const` match
    // keeps this panic-free rather than relying on `expect`.
    None => NonZeroUsize::MIN,
};

/// Maximum total pixel bytes kept per cache, per thread.
const COLR_CACHE_BYTES: usize = 8 * 1024 * 1024;

/// Results larger than this are handed back uncached.
const COLR_CACHE_MAX_ENTRY_BYTES: usize = 2 * 1024 * 1024;

/// Identity of the font a cached glyph came from.
///
/// Sound only because the cache entry keeps the matching [`Arc<[u8]>`] alive;
/// see the module documentation.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct FontKey {
    /// Address of the first byte of the font data.
    addr: usize,
    /// Length in bytes, so that two `Arc`s cannot alias by address alone.
    len: usize,
}

impl FontKey {
    /// Derive the key of a font handle.
    fn of(font_data: &Arc<[u8]>) -> Self {
        Self {
            addr: Arc::as_ptr(font_data) as *const u8 as usize,
            len: font_data.len(),
        }
    }
}

/// Cache key for [`crate::color::render_colr_glyph_sized_cached`].
///
/// `px_bits` is the raw bit pattern of the em size.  Callers reject NaN and
/// non-positive sizes before a key is built, so the bit pattern is a total,
/// exact identity for the sizes that reach the cache.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct SizedKey {
    font: FontKey,
    glyph_id: u16,
    palette: u16,
    px_bits: u32,
}

/// Cache key for [`crate::color::render_colr_cached`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct BitmapKey {
    font: FontKey,
    glyph_id: u16,
    palette: u16,
    width: u32,
    height: u32,
}

/// A cached render plus the font handle that keeps its key valid.
struct Entry<V> {
    /// Retained so the address in the key cannot be recycled.
    _font: Arc<[u8]>,
    /// The rendered glyph.
    value: Arc<V>,
}

/// A cached value that knows how much heap it holds.
trait ByteSized {
    /// Approximate bytes retained by this value, pixels included.
    fn byte_size(&self) -> usize;
}

impl ByteSized for ColorGlyphImage {
    fn byte_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.rgba.len()
    }
}

impl ByteSized for ColorGlyphBitmap {
    fn byte_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.rgba.len()
    }
}

/// An LRU cache bounded by both entry count and total bytes.
struct BoundedCache<K, V> {
    lru: lru::LruCache<K, Entry<V>>,
    bytes: usize,
    hits: u64,
    misses: u64,
}

impl<K: Hash + Eq, V: ByteSized> BoundedCache<K, V> {
    /// Create an empty cache at the module's fixed capacity.
    fn new() -> Self {
        Self {
            lru: lru::LruCache::new(COLR_CACHE_CAP),
            bytes: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Look `key` up, recording a hit or a miss.
    fn get(&mut self, key: &K) -> Option<Arc<V>> {
        match self.lru.get(key) {
            Some(entry) => {
                self.hits += 1;
                Some(Arc::clone(&entry.value))
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Store `value`, evicting until both bounds hold again.
    ///
    /// Values above [`COLR_CACHE_MAX_ENTRY_BYTES`] are dropped on the floor:
    /// the caller already owns the `Arc`, so nothing is lost but the memo.
    fn put(&mut self, key: K, font: &Arc<[u8]>, value: Arc<V>) {
        let size = value.byte_size();
        if size > COLR_CACHE_MAX_ENTRY_BYTES {
            return;
        }
        let entry = Entry {
            _font: Arc::clone(font),
            value,
        };
        // `push` (not `put`) also reports the entry evicted for capacity, which
        // the byte counter has to account for.
        if let Some((_, evicted)) = self.lru.push(key, entry) {
            self.bytes = self.bytes.saturating_sub(evicted.value.byte_size());
        }
        self.bytes = self.bytes.saturating_add(size);
        while self.bytes > COLR_CACHE_BYTES {
            match self.lru.pop_lru() {
                Some((_, evicted)) => {
                    self.bytes = self.bytes.saturating_sub(evicted.value.byte_size());
                }
                None => break,
            }
        }
    }

    /// Drop every entry and reset the counters.
    fn clear(&mut self) {
        self.lru.clear();
        self.bytes = 0;
        self.hits = 0;
        self.misses = 0;
    }
}

thread_local! {
    static SIZED_CACHE: RefCell<BoundedCache<SizedKey, ColorGlyphImage>> =
        RefCell::new(BoundedCache::new());
    static BITMAP_CACHE: RefCell<BoundedCache<BitmapKey, ColorGlyphBitmap>> =
        RefCell::new(BoundedCache::new());
}

/// A snapshot of the calling thread's COLR cache counters.
///
/// Both COLR caches (sized images and fixed-size bitmaps) are summed together.
/// The counters are per-thread, like the caches themselves, and are reset by
/// [`clear_colr_cache`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColrCacheStats {
    /// Lookups served from the cache without re-rendering.
    pub hits: u64,
    /// Lookups that had to paint the glyph.
    pub misses: u64,
    /// Rendered glyphs currently resident.
    pub entries: usize,
    /// Approximate bytes of pixel data currently resident.
    pub bytes: usize,
}

/// Read the calling thread's COLR cache counters.
///
/// Useful to confirm that a render loop is actually hitting the cache:
///
/// ```
/// use oxitext_raster::{clear_colr_cache, colr_cache_stats, ColrCacheStats};
/// clear_colr_cache();
/// assert_eq!(colr_cache_stats(), ColrCacheStats::default());
/// ```
pub fn colr_cache_stats() -> ColrCacheStats {
    let (sized_hits, sized_misses, sized_entries, sized_bytes) = SIZED_CACHE.with(|c| {
        let c = c.borrow();
        (c.hits, c.misses, c.lru.len(), c.bytes)
    });
    let (bitmap_hits, bitmap_misses, bitmap_entries, bitmap_bytes) = BITMAP_CACHE.with(|c| {
        let c = c.borrow();
        (c.hits, c.misses, c.lru.len(), c.bytes)
    });
    ColrCacheStats {
        hits: sized_hits.saturating_add(bitmap_hits),
        misses: sized_misses.saturating_add(bitmap_misses),
        entries: sized_entries.saturating_add(bitmap_entries),
        bytes: sized_bytes.saturating_add(bitmap_bytes),
    }
}

/// Drop every rendered colour glyph cached by the calling thread, release the
/// font handles they retain, and reset the counters reported by
/// [`colr_cache_stats`].
///
/// Rendering is a pure function of the font bytes, glyph id, size and palette,
/// so this is never required for correctness — it exists to release memory and
/// to let a benchmark or test measure a cold run.
pub fn clear_colr_cache() {
    SIZED_CACHE.with(|c| c.borrow_mut().clear());
    BITMAP_CACHE.with(|c| c.borrow_mut().clear());
}

/// Memoized [`crate::color::render_colr_glyph_sized`].
///
/// `render` is called only on a miss, and only its `Some` results are stored:
/// a glyph with no colour data is cheap to reject and caching the negative
/// would need a separate key space.
pub(crate) fn get_or_render_sized<F>(
    font_data: &Arc<[u8]>,
    glyph_id: u16,
    px_per_em: f32,
    palette: u16,
    render: F,
) -> Option<Arc<ColorGlyphImage>>
where
    F: FnOnce() -> Option<ColorGlyphImage>,
{
    let key = SizedKey {
        font: FontKey::of(font_data),
        glyph_id,
        palette,
        px_bits: px_per_em.to_bits(),
    };
    if let Some(hit) = SIZED_CACHE.with(|c| c.borrow_mut().get(&key)) {
        return Some(hit);
    }
    // The cache borrow is released before rendering so that a future painter
    // which itself renders a colour glyph cannot re-enter a live borrow.
    let value = Arc::new(render()?);
    SIZED_CACHE.with(|c| c.borrow_mut().put(key, font_data, Arc::clone(&value)));
    Some(value)
}

/// Memoized fixed-size COLR rendering, backing
/// [`crate::color::render_colr_cached`].
pub(crate) fn get_or_render_bitmap<F>(
    font_data: &Arc<[u8]>,
    glyph_id: u16,
    width: u32,
    height: u32,
    palette: u16,
    render: F,
) -> Option<Arc<ColorGlyphBitmap>>
where
    F: FnOnce() -> Option<ColorGlyphBitmap>,
{
    let key = BitmapKey {
        font: FontKey::of(font_data),
        glyph_id,
        palette,
        width,
        height,
    };
    if let Some(hit) = BITMAP_CACHE.with(|c| c.borrow_mut().get(&key)) {
        return Some(hit);
    }
    let value = Arc::new(render()?);
    BITMAP_CACHE.with(|c| c.borrow_mut().put(key, font_data, Arc::clone(&value)));
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font handle standing in for real font bytes.
    fn font(len: usize) -> Arc<[u8]> {
        vec![0u8; len].into()
    }

    fn image(bytes: usize) -> ColorGlyphImage {
        ColorGlyphImage {
            width: 1,
            height: 1,
            bearing_x: 0,
            bearing_y: 0,
            rgba: vec![7u8; bytes],
        }
    }

    #[test]
    fn font_key_distinguishes_distinct_handles() {
        let a = font(128);
        let b = font(128);
        assert_ne!(FontKey::of(&a), FontKey::of(&b));
        assert_eq!(FontKey::of(&a), FontKey::of(&Arc::clone(&a)));
    }

    #[test]
    fn bounded_cache_counts_hits_and_misses() {
        let f = font(16);
        let mut cache: BoundedCache<u32, ColorGlyphImage> = BoundedCache::new();
        assert!(cache.get(&1).is_none());
        cache.put(1, &f, Arc::new(image(4)));
        assert!(cache.get(&1).is_some());
        assert_eq!((cache.hits, cache.misses), (1, 1));
    }

    #[test]
    fn bounded_cache_retains_the_font_handle() {
        let f = font(16);
        let mut cache: BoundedCache<u32, ColorGlyphImage> = BoundedCache::new();
        cache.put(1, &f, Arc::new(image(4)));
        assert_eq!(Arc::strong_count(&f), 2, "the cache must retain the font");
        cache.clear();
        assert_eq!(Arc::strong_count(&f), 1, "clearing must release the font");
    }

    #[test]
    fn bounded_cache_tracks_bytes_and_evicts_by_bytes() {
        let f = font(16);
        let mut cache: BoundedCache<u32, ColorGlyphImage> = BoundedCache::new();
        // Five 2 MiB entries against an 8 MiB budget: the oldest must go.
        for i in 0..5u32 {
            cache.put(i, &f, Arc::new(image(COLR_CACHE_MAX_ENTRY_BYTES)));
        }
        assert!(
            cache.bytes <= COLR_CACHE_BYTES,
            "byte budget exceeded: {}",
            cache.bytes
        );
        assert!(cache.lru.len() < 5, "nothing was evicted");
    }

    #[test]
    fn bounded_cache_refuses_oversized_entries() {
        let f = font(16);
        let mut cache: BoundedCache<u32, ColorGlyphImage> = BoundedCache::new();
        cache.put(0, &f, Arc::new(image(COLR_CACHE_MAX_ENTRY_BYTES + 1)));
        assert_eq!(cache.lru.len(), 0);
        assert_eq!(cache.bytes, 0);
        assert_eq!(
            Arc::strong_count(&f),
            1,
            "a refused entry retained the font"
        );
    }

    #[test]
    fn bounded_cache_replacement_does_not_double_count() {
        let f = font(16);
        let mut cache: BoundedCache<u32, ColorGlyphImage> = BoundedCache::new();
        cache.put(0, &f, Arc::new(image(1024)));
        let after_first = cache.bytes;
        cache.put(0, &f, Arc::new(image(1024)));
        assert_eq!(cache.bytes, after_first, "replacing an entry leaked bytes");
        assert_eq!(cache.lru.len(), 1);
    }

    #[test]
    fn bounded_cache_clear_resets_everything() {
        let f = font(16);
        let mut cache: BoundedCache<u32, ColorGlyphImage> = BoundedCache::new();
        cache.put(0, &f, Arc::new(image(64)));
        let _ = cache.get(&0);
        cache.clear();
        assert_eq!(cache.lru.len(), 0);
        assert_eq!(cache.bytes, 0);
        assert_eq!((cache.hits, cache.misses), (0, 0));
    }

    #[test]
    fn clear_resets_public_stats() {
        clear_colr_cache();
        assert_eq!(colr_cache_stats(), ColrCacheStats::default());
    }

    #[test]
    fn negative_results_are_not_cached() {
        clear_colr_cache();
        let data: Arc<[u8]> = Arc::from(&b"not a font"[..]);
        let out = get_or_render_sized(&data, 1, 16.0, 0, || None);
        assert!(out.is_none());
        let stats = colr_cache_stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(Arc::strong_count(&data), 1);
    }
}
