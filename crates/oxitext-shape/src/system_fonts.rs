//! System font discovery via `oxifont-db`.
//!
//! This module provides two entry points for loading font bytes from the
//! system font catalog without requiring callers to supply raw `&[u8]`
//! themselves:
//!
//! - [`load_font_for_family`] — resolve a CSS family name (or generic alias
//!   like `"sans-serif"`) to the best-matching font bytes.
//! - [`load_best_font_for_text`] — find the font whose OS/2 Unicode range
//!   bits provide the best coverage for the codepoints in `text`.
//!
//! Both functions perform a **cold scan** of the system font directories on
//! every call; callers that need the database for multiple queries should
//! build it once with [`build_system_db`] and use [`load_font_for_family_from`]
//! / [`load_best_font_for_text_from`] with the shared database.
//!
//! # Note on TTC face indices
//!
//! `oxifont_db::FaceInfo::face_index` may be non-zero for TrueType Collection
//! (`.ttc`) files.  The current implementation always reads the full file bytes
//! and does **not** extract a single sub-face; swash's `FontRef::from_index`
//! is called later by the shaper with index `0`.  For most system fonts (plain
//! `.ttf`/`.otf`) this is correct.
//!
//! Gated behind the `system-fonts` Cargo feature.

use oxifont::db::{FontDatabase, Query, Source};

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Build a [`FontDatabase`] from the system font directories.
///
/// Returns `None` when the OS font directories are inaccessible or yield no
/// parseable fonts (uncommon outside headless CI).
pub fn build_system_db() -> Option<FontDatabase> {
    FontDatabase::system().ok()
}

/// Load raw font bytes for the named CSS family from the system font database.
///
/// `family` may be a concrete family name (e.g. `"Arial"`) or a CSS generic
/// alias (e.g. `"sans-serif"`, `"serif"`, `"monospace"`).  Generic aliases are
/// expanded to an ordered candidate list before the CSS Level 4 matching phase
/// runs (see `oxifont-db`'s Query documentation).
///
/// Returns `None` when no matching font is found or the font file cannot be
/// read.
pub fn load_font_for_family(family: &str) -> Option<Vec<u8>> {
    let db = build_system_db()?;
    load_font_for_family_from(&db, family)
}

/// Load raw font bytes for `family` from an already-constructed [`FontDatabase`].
///
/// Prefer this over [`load_font_for_family`] when you need to query the same
/// database multiple times — it avoids rebuilding the system database on every
/// call.
pub fn load_font_for_family_from(db: &FontDatabase, family: &str) -> Option<Vec<u8>> {
    let face = Query::new(db).family(family).match_best()?;
    load_face_bytes(&face.source)
}

/// Find and load the font with the best Unicode coverage for the given text.
///
/// Uses [`Query::match_with_fallback`] which selects fonts whose OS/2 Unicode
/// range bits cover all codepoints in `text`.  Returns the bytes of the first
/// (highest-quality) match, or `None` if no suitable font is found.
///
/// When `text` is empty the function returns `None` without scanning.
pub fn load_best_font_for_text(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() {
        return None;
    }
    let db = build_system_db()?;
    load_best_font_for_text_from(&db, text)
}

/// Find and load the best-coverage font for `text` from an already-constructed
/// [`FontDatabase`].
///
/// Prefer this over [`load_best_font_for_text`] when the same database is
/// reused for multiple queries.
pub fn load_best_font_for_text_from(db: &FontDatabase, text: &str) -> Option<Vec<u8>> {
    if text.is_empty() {
        return None;
    }
    let faces = Query::new(db).match_with_fallback(text);
    for face in faces {
        if let Some(bytes) = load_face_bytes(&face.source) {
            return Some(bytes);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Read font bytes from the given [`Source`].
///
/// `Source::File` paths are read from disk; `Source::Memory` bytes are cloned
/// directly.  Returns `None` on I/O failure.
fn load_face_bytes(source: &Source) -> Option<Vec<u8>> {
    match source {
        Source::File(path) => std::fs::read(path).ok(),
        Source::Memory(data) => Some(data.clone()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that `load_font_for_family` returns a valid font or `None` for
    /// the `"sans-serif"` generic family.  Both outcomes are acceptable because
    /// headless CI may not have any system fonts installed.
    #[test]
    fn load_font_for_family_sans_serif() {
        if let Some(bytes) = load_font_for_family("sans-serif") {
            assert!(bytes.len() > 100, "font data should be non-trivial in size");
            // OTF/TTF magic: 0x00010000 (TrueType), OTTO (CFF OpenType),
            //                'true', or 'ttcf' (TTC collection).
            let magic_ok = matches!(
                bytes[..4],
                [0x00, 0x01, 0x00, 0x00]
                    | [0x4F, 0x54, 0x54, 0x4F]
                    | [0x74, 0x72, 0x75, 0x65]
                    | [0x74, 0x74, 0x63, 0x66]
            );
            assert!(magic_ok, "font data must begin with valid font magic bytes");
        }
        // None is acceptable on headless CI with no system fonts.
    }

    /// Verify that `load_best_font_for_text` returns a valid font for ASCII or
    /// `None` when no system fonts are available.
    #[test]
    fn load_best_font_for_ascii_text() {
        if let Some(bytes) = load_best_font_for_text("Hello World") {
            assert!(bytes.len() > 100, "font data should be non-trivial in size");
        }
        // None is acceptable on headless CI with no system fonts.
    }

    /// Verify that passing empty text returns `None` without panicking.
    #[test]
    fn load_best_font_for_empty_text_returns_none() {
        let result = load_best_font_for_text("");
        assert!(result.is_none(), "empty text must return None");
    }

    /// Verify that the `*_from` variants accept a pre-built database.
    #[test]
    fn from_variants_accept_pre_built_db() {
        let db = match build_system_db() {
            Some(db) => db,
            None => return, // no system fonts available — skip
        };
        // Neither call should panic regardless of font availability.
        let _family = load_font_for_family_from(&db, "sans-serif");
        let _text = load_best_font_for_text_from(&db, "abc");
    }

    /// Verify that the `*_from` variants with empty text return `None`.
    #[test]
    fn from_variant_empty_text_returns_none() {
        let db = match build_system_db() {
            Some(db) => db,
            None => return,
        };
        assert!(load_best_font_for_text_from(&db, "").is_none());
    }
}
