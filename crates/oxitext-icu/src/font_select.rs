//! Locale-aware font family selection via `oxifont-db`.
//!
//! This module bridges the ICU locale machinery in `oxitext-icu` with the
//! CSS-Level-4 indexed font database from `oxifont-db`.  Given a BCP-47 locale
//! tag (e.g. `"ja-JP"`, `"zh-CN"`, `"ar-SA"`) it resolves the most appropriate
//! system font family for that locale, taking into account:
//!
//! * **Locale-specific name table records** — OpenType fonts carry per-LCID
//!   family name strings in their `name` table.  `oxifont-db` reads these at
//!   load time and stores them as `FaceInfo::locale_families`.
//! * **Script-aware CSS matching** — the generic alias table in `oxifont-db`
//!   maps CSS generics (`"sans-serif"`, `"sans-serif-cjk"`, etc.) to concrete
//!   ordered candidate lists.  Script-appropriate generics are derived from the
//!   BCP-47 language subtag.
//! * **Locale-preference query** — `Query::locale(bcp47)` biases the CSS match
//!   towards faces whose locale-specific name equals the result from
//!   `FaceInfo::family_for_locale`.
//!
//! # Feature gate
//!
//! This module is compiled only when the `fonts` Cargo feature is enabled.
//!
//! # Example
//!
//! ```no_run
//! # #[cfg(feature = "fonts")]
//! # {
//! use oxitext_icu::font_select::LocaleFontSelector;
//!
//! let selector = LocaleFontSelector::from_system().expect("system font database");
//!
//! // Resolve the best sans-serif family for Japanese.
//! if let Some(name) = selector.family_for_locale("ja-JP") {
//!     println!("Japanese sans-serif: {name}");
//! }
//!
//! // Resolve with explicit generic and weight.
//! if let Some(name) = selector.query_family("ja-JP", "sans-serif", 400) {
//!     println!("Japanese sans-serif w400: {name}");
//! }
//! # }
//! ```

use oxifont::db::{FontDatabase, Query};

// ---------------------------------------------------------------------------
// Locale → CSS generic mapping
// ---------------------------------------------------------------------------

/// Returns the CSS generic family most appropriate for the given BCP-47 locale.
///
/// The primary language subtag (before the first `-`) is used for the decision
/// so that both `"zh"` and `"zh-CN"` map to the same generic.
///
/// | Language | Generic returned |
/// |-----------|-----------------|
/// | `ja` | `"sans-serif-cjk"` |
/// | `zh` | `"sans-serif-cjk"` |
/// | `ko` | `"sans-serif-cjk"` |
/// | `ar`, `fa`, `ur` | `"sans-serif"` |
/// | *(all others)* | `"sans-serif"` |
fn locale_to_generic(bcp47: &str) -> &'static str {
    let lang = bcp47.split('-').next().unwrap_or(bcp47);
    match lang {
        "ja" | "zh" | "ko" => "sans-serif-cjk",
        _ => "sans-serif",
    }
}

// ---------------------------------------------------------------------------
// LocaleFontSelector
// ---------------------------------------------------------------------------

/// Locale-aware font family selector backed by an `oxifont-db` [`FontDatabase`].
///
/// Constructed once from the system font catalog (or any pre-built database)
/// and then queried many times without I/O.  All resolution is in-memory after
/// construction.
///
/// # Thread safety
///
/// `LocaleFontSelector` contains an `Arc`-backed [`FontDatabase`] and is
/// therefore `Send + Sync` once constructed.
pub struct LocaleFontSelector {
    db: FontDatabase,
}

impl LocaleFontSelector {
    /// Build a selector from the system font catalog.
    ///
    /// Performs a directory scan of the OS font directories.  The scan is
    /// synchronous; for non-blocking use consider calling
    /// [`FontDatabase::load_system_fonts_bg`] directly and constructing via
    /// [`Self::from_db`] once the background thread completes.
    ///
    /// # Errors
    ///
    /// Returns a `String` error description when the system font directories
    /// cannot be scanned (rare; normally signals a very minimal or sandboxed
    /// environment).
    pub fn from_system() -> Result<Self, String> {
        let db = FontDatabase::system().map_err(|e| e.to_string())?;
        Ok(Self { db })
    }

    /// Build a selector from a pre-constructed [`FontDatabase`].
    ///
    /// Use this when the database has already been loaded (e.g. via a shared
    /// cache) to avoid redundant I/O.
    pub fn from_db(db: FontDatabase) -> Self {
        Self { db }
    }

    /// Returns a reference to the underlying database.
    pub fn database(&self) -> &FontDatabase {
        &self.db
    }

    /// Resolve the canonical family name for the best font matching `bcp47`.
    ///
    /// The generic CSS family is derived automatically from the locale's
    /// language subtag.  Weight 400 (Regular) is
    /// used and the result is the English family name of the winning face.
    ///
    /// Returns `None` when the system has no fonts installed or none match the
    /// derived generic.
    pub fn family_for_locale(&self, bcp47: &str) -> Option<String> {
        let generic = locale_to_generic(bcp47);
        self.query_family(bcp47, generic, 400)
    }

    /// Resolve the locale-specific display name for the best font matching
    /// `bcp47`.
    ///
    /// Identical to [`Self::family_for_locale`] but returns the localised
    /// family name string from the font's `name` table (via
    /// `FaceInfo::family_for_locale`) rather than the canonical ASCII name.
    /// For fonts that lack a localised name record the canonical name is
    /// returned instead.
    ///
    /// Returns `None` when no matching face is found.
    pub fn locale_name_for_locale(&self, bcp47: &str) -> Option<String> {
        let generic = locale_to_generic(bcp47);
        let face = Query::new(&self.db)
            .family(generic)
            .weight(400)
            .locale(bcp47)
            .match_best()?;
        Some(face.family_for_locale(bcp47).to_owned())
    }

    /// Resolve the family name for a given locale, explicit CSS generic, and
    /// font weight.
    ///
    /// Runs the full CSS Fonts Level 4 matching algorithm with the locale
    /// preference bias.  The returned `String` is the winning face's canonical
    /// family name.
    ///
    /// # Arguments
    ///
    /// * `bcp47`   — BCP-47 locale tag, e.g. `"ja-JP"`, `"zh-CN"`, `"en-US"`.
    /// * `generic` — CSS generic family, e.g. `"sans-serif"`, `"serif"`,
    ///   `"monospace"`, or the extended CJK generics
    ///   `"sans-serif-cjk"`, `"serif-cjk"`.
    /// * `weight`  — CSS weight value in the range 100..=900.
    ///
    /// Returns `None` when no matching face is found in the database.
    pub fn query_family(&self, bcp47: &str, generic: &str, weight: u16) -> Option<String> {
        let face = Query::new(&self.db)
            .family(generic)
            .weight(weight)
            .locale(bcp47)
            .match_best()?;
        Some(face.family.clone())
    }

    /// Returns all candidate family names for the given locale and generic,
    /// ordered by CSS Level 4 preference.
    ///
    /// This is the multi-result variant of [`Self::query_family`]; it is useful
    /// for building font fallback lists in a rendering pipeline.
    ///
    /// Deduplicates by family name so that multiple faces (e.g. Regular and
    /// Bold) from the same family appear only once.
    pub fn families_for_locale(&self, bcp47: &str, generic: &str, weight: u16) -> Vec<String> {
        let faces = Query::new(&self.db)
            .family(generic)
            .weight(weight)
            .locale(bcp47)
            .match_all();

        let mut seen = std::collections::HashSet::new();
        let mut names = Vec::new();
        for face in faces {
            if seen.insert(face.family.to_lowercase()) {
                names.push(face.family.clone());
            }
        }
        names
    }

    /// Returns the preferred locale-specific family name for each of the given
    /// BCP-47 locales, applying the default generic for each locale.
    ///
    /// This is a convenience batch resolver for pipelines that manage multiple
    /// simultaneous locales (e.g. multi-lingual documents).
    ///
    /// Each entry in the returned `Vec` corresponds positionally to the input
    /// `locales` slice.  `None` entries indicate that no suitable font was found
    /// for that locale.
    pub fn batch_resolve(&self, locales: &[&str]) -> Vec<Option<String>> {
        locales
            .iter()
            .map(|&bcp47| self.family_for_locale(bcp47))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// `locale_to_generic` must return CJK generics for CJK locales and the
    /// default `"sans-serif"` for everything else.
    #[test]
    fn locale_to_generic_cjk() {
        assert_eq!(locale_to_generic("ja"), "sans-serif-cjk");
        assert_eq!(locale_to_generic("ja-JP"), "sans-serif-cjk");
        assert_eq!(locale_to_generic("zh"), "sans-serif-cjk");
        assert_eq!(locale_to_generic("zh-CN"), "sans-serif-cjk");
        assert_eq!(locale_to_generic("zh-TW"), "sans-serif-cjk");
        assert_eq!(locale_to_generic("ko"), "sans-serif-cjk");
        assert_eq!(locale_to_generic("ko-KR"), "sans-serif-cjk");
    }

    #[test]
    fn locale_to_generic_non_cjk() {
        assert_eq!(locale_to_generic("en"), "sans-serif");
        assert_eq!(locale_to_generic("en-US"), "sans-serif");
        assert_eq!(locale_to_generic("de-DE"), "sans-serif");
        assert_eq!(locale_to_generic("ar-SA"), "sans-serif");
        assert_eq!(locale_to_generic("ru-RU"), "sans-serif");
    }

    /// `from_system()` must either succeed or return a sensible error on
    /// headless CI.  We do not assert the database is non-empty because CI
    /// containers may have no system fonts.
    #[test]
    fn from_system_does_not_panic() {
        let _result = LocaleFontSelector::from_system();
        // Both Ok and Err are acceptable — we just ensure no panic.
    }

    /// `family_for_locale` returns `Some` or `None` without panicking.
    #[test]
    fn family_for_locale_no_panic() {
        let Ok(sel) = LocaleFontSelector::from_system() else {
            return; // no system fonts — skip
        };
        let _ja = sel.family_for_locale("ja-JP");
        let _en = sel.family_for_locale("en-US");
        let _zh = sel.family_for_locale("zh-CN");
    }

    /// `locale_name_for_locale` returns `Some` or `None` without panicking.
    #[test]
    fn locale_name_for_locale_no_panic() {
        let Ok(sel) = LocaleFontSelector::from_system() else {
            return;
        };
        let _name = sel.locale_name_for_locale("ja-JP");
    }

    /// `query_family` accepts all standard CSS generics without panicking.
    #[test]
    fn query_family_standard_generics() {
        let Ok(sel) = LocaleFontSelector::from_system() else {
            return;
        };
        for generic in &["sans-serif", "serif", "monospace", "cursive", "fantasy"] {
            let _r = sel.query_family("en-US", generic, 400);
        }
    }

    /// `families_for_locale` returns a deduplicated list.
    #[test]
    fn families_for_locale_deduped() {
        let Ok(sel) = LocaleFontSelector::from_system() else {
            return;
        };
        let names = sel.families_for_locale("en-US", "sans-serif", 400);
        // No duplicates (case-insensitive).
        let lower: std::collections::HashSet<String> =
            names.iter().map(|n| n.to_lowercase()).collect();
        assert_eq!(
            lower.len(),
            names.len(),
            "families_for_locale must deduplicate"
        );
    }

    /// `batch_resolve` output length must equal input length.
    #[test]
    fn batch_resolve_length_matches_input() {
        let Ok(sel) = LocaleFontSelector::from_system() else {
            return;
        };
        let locales = ["en-US", "ja-JP", "zh-CN", "de-DE"];
        let results = sel.batch_resolve(&locales);
        assert_eq!(results.len(), locales.len());
    }

    /// `from_db` with an empty database returns `None` for all queries.
    #[test]
    fn from_db_empty_returns_none() {
        let empty_db = FontDatabase::new();
        let sel = LocaleFontSelector::from_db(empty_db);
        assert!(sel.family_for_locale("en-US").is_none());
        assert!(sel.locale_name_for_locale("ja-JP").is_none());
        assert!(sel.query_family("zh-CN", "sans-serif", 400).is_none());
        assert!(sel
            .families_for_locale("ko-KR", "sans-serif", 400)
            .is_empty());
        assert_eq!(sel.batch_resolve(&["en-US", "ja-JP"]), vec![None, None]);
    }

    /// `database()` returns the same db used at construction.
    #[test]
    fn database_accessor() {
        let db = FontDatabase::new();
        let sel = LocaleFontSelector::from_db(db);
        let stats = sel.database().stats();
        assert_eq!(stats.face_count, 0);
    }
}
