//! Thread-local fontdue font cache.
//!
//! Provides [`get_or_parse_fontdue`], which caches [`fontdue::Font`] instances
//! in a [`std::thread::LocalKey`]-backed `RefCell<LruCache>`.  Callers in
//! hot multi-threaded rendering loops avoid the lock contention of the
//! global [`crate::backend::FontdueRaster`] cache.
//!
//! The cache key is a 64-bit FNV-1a hash of the first 64 bytes of the font
//! data — a cheap identity approximation suitable for distinguishing different
//! font files without hashing the entire file.
//!
//! # Why `Arc`
//!
//! Cached fonts are handed out as [`Arc<fontdue::Font>`], never as owned
//! `fontdue::Font` values.  A `fontdue::Font` owns every parsed glyph outline
//! in the face, so cloning one deep-copies the whole font: for a CJK face with
//! tens of thousands of glyphs that costs hundreds of milliseconds *per glyph
//! rasterization*, which defeats the purpose of the cache entirely.  Cloning
//! the `Arc` is a single refcount increment instead.

use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::sync::Arc;

/// Per-thread LRU capacity for parsed `fontdue::Font` instances.
const TL_CACHE_CAP: usize = 32;

thread_local! {
    static TL_FONT_CACHE: RefCell<lru::LruCache<u64, Arc<fontdue::Font>>> = RefCell::new(
        // SAFETY: TL_CACHE_CAP is a non-zero compile-time constant.
        lru::LruCache::new(NonZeroUsize::new(TL_CACHE_CAP).expect("TL_CACHE_CAP is non-zero")),
    );
}

/// Hash the first 64 bytes of font data as a cheap identity key.
///
/// Uses FNV-1a (64-bit) for speed.  The assumption is that distinct font
/// files differ within their first 64 bytes — this holds for all common font
/// formats (TTF, OTF, WOFF2) because the header differs.
fn font_data_key(data: &[u8]) -> u64 {
    // FNV-1a 64-bit — offset basis and prime from the FNV spec.
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x00000100000001b3;

    let sample = &data[..data.len().min(64)];
    let mut h: u64 = OFFSET_BASIS;
    for &b in sample {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Get or create a shared [`fontdue::Font`] handle in the thread-local cache.
///
/// On the first call for a given font (identified by a hash of its first 64
/// bytes), the font is parsed via [`fontdue::Font::from_bytes`] and stored.
/// Subsequent calls on the same thread return another [`Arc`] handle to the
/// *same* parsed instance — a refcount bump, not a copy of the glyph tables.
///
/// `Arc<fontdue::Font>` derefs to `fontdue::Font`, so the returned handle can
/// be used directly for rasterization:
///
/// ```no_run
/// # let face_data: &[u8] = &[];
/// if let Some(font) = oxitext_raster::get_or_parse_fontdue(face_data) {
///     let (metrics, coverage) = font.rasterize_indexed(36, 16.0);
///     let _ = (metrics, coverage);
/// }
/// ```
///
/// Returns `None` if the font data is empty or fontdue fails to parse it.
pub fn get_or_parse_fontdue(face_data: &[u8]) -> Option<Arc<fontdue::Font>> {
    if face_data.is_empty() {
        return None;
    }

    let key = font_data_key(face_data);

    TL_FONT_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        // LruCache has no entry() API — use get + put.
        if let Some(font) = c.get(&key) {
            return Some(Arc::clone(font));
        }
        let font =
            Arc::new(fontdue::Font::from_bytes(face_data, fontdue::FontSettings::default()).ok()?);
        c.put(key, Arc::clone(&font));
        Some(font)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_data_returns_none() {
        assert!(get_or_parse_fontdue(&[]).is_none());
    }

    #[test]
    fn invalid_data_returns_none() {
        assert!(get_or_parse_fontdue(b"not a font file at all xxxx").is_none());
    }

    #[test]
    fn font_data_key_stable() {
        let data = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let k1 = font_data_key(data);
        let k2 = font_data_key(data);
        assert_eq!(k1, k2);
    }

    #[test]
    fn font_data_key_differs_for_different_data() {
        let k1 = font_data_key(b"AAAAAAAAAA");
        let k2 = font_data_key(b"BBBBBBBBBB");
        assert_ne!(k1, k2);
    }

    /// Repeated lookups must hand back the *same* parsed instance rather than
    /// a deep copy — this is the regression guard for the CJK rasterization
    /// slowdown fixed in 0.2.1.
    #[test]
    fn repeated_lookups_share_one_parsed_font() {
        let data = oxifont_bundled::NOTO_SANS_REGULAR;
        let a = get_or_parse_fontdue(data).expect("bundled Noto Sans must parse");
        let b = get_or_parse_fontdue(data).expect("bundled Noto Sans must parse");
        assert!(
            Arc::ptr_eq(&a, &b),
            "thread-local cache must return shared handles, not deep clones"
        );
    }

    /// Each thread keeps its own cache, but every handle within a thread is
    /// shared and every thread's font rasterizes identically.
    #[test]
    fn per_thread_caches_are_independent_but_equivalent() {
        let data = oxifont_bundled::NOTO_SANS_REGULAR;
        let main = get_or_parse_fontdue(data).expect("bundled Noto Sans must parse");
        let (main_metrics, main_coverage) = main.rasterize_indexed(36, 24.0);

        let handle = std::thread::spawn(move || {
            let other = get_or_parse_fontdue(oxifont_bundled::NOTO_SANS_REGULAR)
                .expect("bundled Noto Sans must parse");
            let (metrics, coverage) = other.rasterize_indexed(36, 24.0);
            (metrics.width, metrics.height, coverage)
        });
        let (w, h, coverage) = handle.join().expect("worker thread must not panic");

        assert_eq!(w, main_metrics.width);
        assert_eq!(h, main_metrics.height);
        assert_eq!(coverage, main_coverage);
    }
}
