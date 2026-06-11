//! On-the-fly font subsetting for PDF text rendering pipelines.
//!
//! This module integrates [`oxifont_subset::pdf_subset::PdfFontSubsetter`] with
//! the oxitext text rendering pipeline.  A `TextFontSubsetter` is created once
//! per font per PDF document.  As each page is composed — using the oxitext
//! `Pipeline` or manual shaping — the caller feeds rendered text through
//! `TextFontSubsetter::feed_text` or individual codepoints / GIDs through the
//! corresponding methods.  When all pages have been composed,
//! `TextFontSubsetter::finalize` produces a minimal subset font containing
//! exactly the glyphs that were referenced.
//!
//! # Feature flag
//!
//! This module is only available when the `font-subset` Cargo feature is
//! enabled:
//!
//! ```toml
//! oxitext = { version = "0.1.2", features = ["font-subset"] }
//! ```
//!
//! # Example
//!
//! ```no_run
//! use oxitext::pdf_subset::TextFontSubsetter;
//!
//! // Load the font once for the whole document.
//! let font_bytes = std::fs::read("NotoSans-Regular.ttf").expect("read font");
//!
//! let mut subsetter = TextFontSubsetter::for_pdf(font_bytes);
//!
//! // Accumulate all text that will appear in the PDF.
//! subsetter.feed_text("Hello, world!");
//! subsetter.feed_text("Page 2 content: αβγδ");
//!
//! // After all pages are composed, produce the minimal subset.
//! let (subset_bytes, stats) = subsetter.finalize().expect("subset failed");
//! println!(
//!     "Original {} B → subset {} B, {} glyphs retained",
//!     stats.original_size, stats.subset_size, stats.glyphs_retained
//! );
//! // Embed `subset_bytes` into the PDF stream.
//! ```
//!
//! # Multi-threaded page composition
//!
//! For parallel PDF renderers, create one `TextFontSubsetter` per thread and
//! merge them into a single accumulator before finalizing:
//!
//! ```no_run
//! use oxitext::pdf_subset::TextFontSubsetter;
//!
//! let font_bytes = std::fs::read("font.ttf").expect("read font");
//!
//! let mut thread1 = TextFontSubsetter::for_pdf(font_bytes.clone());
//! let mut thread2 = TextFontSubsetter::for_pdf(font_bytes);
//!
//! thread1.feed_text("Page 1 text");
//! thread2.feed_text("Page 2 text");
//!
//! // Merge thread2 into thread1 (thread2 is reset after merge).
//! thread1.merge(&mut thread2);
//!
//! let (subset_bytes, _stats) = thread1.finalize().expect("subset failed");
//! ```

use std::collections::BTreeSet;

use oxifont_subset::pdf_subset::PdfFontSubsetter;

// These are re-exported at the module level at the bottom of this file.
pub use oxifont_subset::pdf_subset::PdfSubsetResult;
pub use oxifont_subset::{SubsetError, SubsetOptions, SubsetStats};

// ---------------------------------------------------------------------------
// TextFontSubsetter
// ---------------------------------------------------------------------------

/// On-the-fly font subsetter for PDF text rendering.
///
/// Accumulates Unicode codepoints and raw GIDs across multiple text placement
/// operations — typically spanning multiple pages of a PDF document — and
/// produces a minimal subset font on [`TextFontSubsetter::finalize`].
///
/// `TextFontSubsetter` is a thin ergonomic wrapper around
/// [`oxifont_subset::pdf_subset::PdfFontSubsetter`] that is pre-wired with
/// the text-oriented API surface expected by PDF composition code.
///
/// # Thread safety
///
/// `TextFontSubsetter` is **not** `Sync`.  For parallel PDF renderers, create
/// one instance per thread and call [`merge`] before finalization.
///
/// [`merge`]: TextFontSubsetter::merge
#[derive(Debug)]
pub struct TextFontSubsetter {
    inner: PdfFontSubsetter,
}

impl TextFontSubsetter {
    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    /// Create a new accumulator with a fully customisable [`SubsetOptions`].
    ///
    /// Use [`TextFontSubsetter::for_pdf`] or [`TextFontSubsetter::for_web`] for
    /// the common presets.
    pub fn new(font_data: Vec<u8>, opts: SubsetOptions) -> Self {
        Self {
            inner: PdfFontSubsetter::new(font_data, opts),
        }
    }

    /// Create a new accumulator using the PDF subsetting preset.
    ///
    /// Preserves TrueType hint tables and the full `name` table, which are
    /// required for high-quality PDF rendering with some PDF viewers.
    ///
    /// Equivalent to:
    /// ```no_run
    /// use oxitext::pdf_subset::TextFontSubsetter;
    /// use oxifont_subset::SubsetOptions;
    ///
    /// let font_bytes = vec![];
    /// let opts = SubsetOptions::default()
    ///     .strip_hints(false)
    ///     .retain_names(true);
    /// let _ = TextFontSubsetter::new(font_bytes, opts);
    /// ```
    pub fn for_pdf(font_data: Vec<u8>) -> Self {
        Self {
            inner: PdfFontSubsetter::for_pdf(font_data),
        }
    }

    /// Create a new accumulator using the web/WOFF2 subsetting preset.
    ///
    /// Strips TrueType hint tables and trims the `name` table to IDs 0–6,
    /// producing a smaller output suitable for web font delivery (e.g. when
    /// the subset font will subsequently be WOFF2-compressed).
    pub fn for_web(font_data: Vec<u8>) -> Self {
        Self {
            inner: PdfFontSubsetter::for_web(font_data),
        }
    }

    // -----------------------------------------------------------------------
    // Accumulation API
    // -----------------------------------------------------------------------

    /// Register every Unicode codepoint in `text` for inclusion in the subset.
    ///
    /// This is the primary accumulation method for text rendering pipelines.
    /// Call it for every string of text that will appear in the document,
    /// regardless of font size or position.
    ///
    /// Internally iterates over Unicode scalar values (not bytes), so all
    /// multi-byte UTF-8 sequences are handled correctly.
    pub fn feed_text(&mut self, text: &str) {
        self.inner.add_text(text);
    }

    /// Register a single Unicode codepoint for inclusion in the subset.
    ///
    /// No-op if `cp` has already been added.
    #[inline]
    pub fn feed_char(&mut self, cp: char) {
        self.inner.add_codepoint(cp);
    }

    /// Register a raw Glyph ID for inclusion in the subset.
    ///
    /// Raw GIDs bypass the `cmap` scan.  The resulting subset font's `cmap`
    /// will **not** map any Unicode codepoint to these GIDs unless the same
    /// codepoints are also registered via [`TextFontSubsetter::feed_text`] / [`TextFontSubsetter::feed_char`].
    /// This is the correct behaviour for PDF CIDFont workflows where text
    /// extraction is handled externally via a ToUnicode CMap.
    #[inline]
    pub fn feed_gid(&mut self, gid: u16) {
        self.inner.add_gid(gid);
    }

    /// Register a slice of raw Glyph IDs.
    ///
    /// Convenience batch form of [`feed_gid`].
    ///
    /// [`feed_gid`]: TextFontSubsetter::feed_gid
    pub fn feed_gids(&mut self, gids: &[u16]) {
        self.inner.add_gids(gids);
    }

    // -----------------------------------------------------------------------
    // Inspection
    // -----------------------------------------------------------------------

    /// Returns the number of distinct codepoints accumulated so far.
    pub fn codepoint_count(&self) -> usize {
        self.inner.codepoint_count()
    }

    /// Returns the number of distinct raw GIDs accumulated so far.
    ///
    /// Does not include GIDs that will be resolved from codepoints at
    /// finalization time.
    pub fn gid_count(&self) -> usize {
        self.inner.gid_count()
    }

    /// Returns `true` if no codepoints or GIDs have been accumulated yet.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns a reference to the accumulated codepoint set.
    pub fn codepoints(&self) -> &BTreeSet<char> {
        self.inner.codepoints()
    }

    /// Returns a reference to the accumulated raw GID set.
    pub fn raw_gids(&self) -> &BTreeSet<u16> {
        self.inner.raw_gids()
    }

    // -----------------------------------------------------------------------
    // Merge
    // -----------------------------------------------------------------------

    /// Merge the accumulated codepoints and GIDs from `other` into `self`.
    ///
    /// After merging, `other` is reset to an empty accumulation state (its
    /// `font_data` and `opts` are preserved for reuse).
    ///
    /// Use this when composing a multi-threaded PDF renderer where each thread
    /// tracks its own glyph usage.
    ///
    /// # Logical contract
    ///
    /// Merging accumulators built with **different** font data or subsetting
    /// options is a logical error.  The merged result will use `self`'s font
    /// data and options; glyphs from `other` will be looked up against `self`'s
    /// font.
    pub fn merge(&mut self, other: &mut Self) {
        self.inner.merge(&mut other.inner);
    }

    // -----------------------------------------------------------------------
    // Finalize
    // -----------------------------------------------------------------------

    /// Produce the minimal subset font from all accumulated codepoints and GIDs.
    ///
    /// Steps performed:
    /// 1. Parse the stored font's `cmap` table.
    /// 2. Resolve accumulated codepoints → old GIDs via `cmap`.
    /// 3. Include accumulated raw GIDs directly.
    /// 4. Always include `.notdef` (GID 0).
    /// 5. Run the full table rewriting pipeline with the configured
    ///    [`SubsetOptions`].
    ///
    /// Returns `(subset_bytes, stats)`.  The accumulator is **not** reset — call
    /// [`reset`] if you need to reuse it for a new document.
    ///
    /// # Errors
    ///
    /// Returns [`SubsetError`] if the stored font data is structurally invalid
    /// or a required table is absent.
    ///
    /// [`reset`]: TextFontSubsetter::reset
    pub fn finalize(&self) -> Result<(Vec<u8>, SubsetStats), SubsetError> {
        self.inner.finalize()
    }

    /// Produce a [`PdfSubsetResult`] combining subset bytes and statistics.
    ///
    /// Convenience wrapper around [`finalize`] that bundles the output into a
    /// single struct.
    ///
    /// [`finalize`]: TextFontSubsetter::finalize
    pub fn finalize_into_result(&self) -> Result<PdfSubsetResult, SubsetError> {
        self.inner.finalize_into_result()
    }

    // -----------------------------------------------------------------------
    // Reset
    // -----------------------------------------------------------------------

    /// Clear the accumulated codepoints and GIDs, ready to subset a new document.
    ///
    /// The `font_data` and `opts` are preserved.
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

// Re-exports declared at the top of the file alongside the `use` statements.
