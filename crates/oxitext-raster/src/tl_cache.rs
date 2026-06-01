//! Thread-local fontdue font cache.
//!
//! Provides [`get_or_parse_fontdue`], which caches [`fontdue::Font`] instances
//! in a [`std::thread::LocalKey`]-backed `RefCell<HashMap>`.  Callers in
//! hot multi-threaded rendering loops avoid the `RwLock` contention of the
//! global [`crate::backend::FontdueRaster`] cache.
//!
//! The cache key is a 64-bit FNV-1a hash of the first 64 bytes of the font
//! data — a cheap identity approximation suitable for distinguishing different
//! font files without hashing the entire file.

use std::cell::RefCell;
use std::num::NonZeroUsize;

/// Per-thread LRU capacity for parsed `fontdue::Font` instances.
const TL_CACHE_CAP: usize = 32;

thread_local! {
    static TL_FONT_CACHE: RefCell<lru::LruCache<u64, fontdue::Font>> = RefCell::new(
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

/// Get or create a [`fontdue::Font`] in the thread-local cache.
///
/// On the first call for a given font (identified by a hash of its first 64
/// bytes), the font is parsed via [`fontdue::Font::from_bytes`] and stored.
/// Subsequent calls on the same thread return a clone of the cached instance
/// at negligible cost.
///
/// Returns `None` if the font data is empty or fontdue fails to parse it.
pub fn get_or_parse_fontdue(face_data: &[u8]) -> Option<fontdue::Font> {
    if face_data.is_empty() {
        return None;
    }

    let key = font_data_key(face_data);

    TL_FONT_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        // LruCache has no entry() API — use contains + put.
        if !c.contains(&key) {
            let font =
                fontdue::Font::from_bytes(face_data, fontdue::FontSettings::default()).ok()?;
            c.put(key, font);
        }
        c.get(&key).cloned()
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
}
