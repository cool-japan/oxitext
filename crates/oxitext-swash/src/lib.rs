/*!
Font introspection, complex text shaping and glyph rendering.

# Provenance

**This crate is a vendored fork of [`swash`](https://github.com/dfrg/swash)
0.2.10 by Chad Brokaw, not an original OxiText crate.** It exists to carry a fix
for two Indic reordering defects — a silent glyph corruption and an
`index out of bounds` panic, upstream issue
[dfrg/swash#93](https://github.com/dfrg/swash/issues/93), open since 2025-04-20 —
that block correct Devanagari text in OxiText and its downstreams. Upstream is
dormant ([#107](https://github.com/dfrg/swash/issues/107)), and
`src/shape/buffer.rs` has had no substantive change since 2021, so there was
nothing to wait for.

Upstream offers this work under `Apache-2.0 OR MIT`; OxiText elects and
redistributes it under the Apache-2.0 arm. Every modified file carries an
`OXITEXT MODIFICATION` header per Apache-2.0 §4(b); the other 37 of 61 source
files are byte-identical to upstream. `PROVENANCE.md` in the crate root is the
full audit trail — the per-file divergence table, the evidence for the fix, the
inherited-`unsafe` inventory and the known-unfixed upstream defects.

# Safety

Unlike every other crate in the OxiText workspace, this one does **not** carry
`#![forbid(unsafe_code)]`. It inherits 54 `unsafe` sites (37 blocks, 17
`unsafe fn`) across 8 files from upstream, essentially all unchecked reads over
untrusted font bytes in the table parsers. None of that code was written or
rewritten by OxiText: the absorption's whole claim is that nothing changed except
the fixes. De-unsafing `internal/parse.rs` and adding
`clippy::undocumented_unsafe_blocks` / `unsafe_op_in_unsafe_fn` are tracked as
0.2.3 work. Treat font data reaching this crate as untrusted input.

For a comprehensive list of features provided by this crate, please check out
the [readme](https://github.com/dfrg/swash/blob/main/README.md) on GitHub.

# Note

This is a low level library focusing on implementations of OpenType and
various related Unicode specifications for building high quality, high performance
text layout and rendering components with minimal overhead.

If you're looking for something higher level, please stay tuned-- work is in
progress.

# Usage

The primary currency in this crate is the [`FontRef`] struct so you'll want to
start there to learn how to construct and use fonts.

Documentation for [shaping](shape) and [scaling](scale) is provided in
the respective modules.
*/

// OXITEXT MODIFICATION (oxitext 0.2.2): upstream carried five crate-level
// clippy allows. Four are gone -- `float_cmp` and `many_single_char_names` were
// vestigial (measured: zero firings), and `needless_lifetimes` /
// `redundant_static_lifetimes` were machine-fixable, so the code was fixed
// instead of silenced. Two remain, each for a stated reason. There is no
// `#[allow]` wall and no `-A clippy::all`. See ../PROVENANCE.md.
//
// 7 sites in the OpenType layout engine's mutually recursive lookup appliers,
// where the argument list *is* the shaping state and threading it through a
// struct would obscure the correspondence with the OpenType spec.
#![allow(clippy::too_many_arguments)]
// A font parser reads untrusted bytes; CONTRIBUTING.md forbids panicking on
// them outside test code. All 13 upstream `.unwrap()` sites were converted, and
// this gate keeps them converted.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(any(test, feature = "std")), no_std)]

// OXITEXT MODIFICATION (oxitext 0.2.2): name the broken feature combination.
// Upstream's float maths comes from `std` or from `core_maths` via `libm`;
// with neither, the build fails deep inside `read-fonts` with an error that
// says nothing about features. See ../PROVENANCE.md.
#[cfg(not(any(feature = "std", feature = "libm")))]
compile_error!("oxitext-swash requires exactly one of the `std` or `libm` features");

extern crate alloc;

#[cfg(feature = "scale")]
pub use zeno;

#[macro_use]
mod macros;

mod attributes;
mod cache;
mod charmap;
mod feature;
mod font;
mod internal;
mod metrics;
mod palette;
mod setting;
mod strike;
mod string;
mod tag;
mod variation;

#[cfg(feature = "scale")]
pub mod scale;

pub mod shape;
pub mod text;

pub use attributes::*;
pub use cache::CacheKey;
pub use charmap::Charmap;
pub use feature::{Action, Feature, WritingSystem};
pub use font::{FontDataRef, FontRef, TableProvider};
pub use metrics::{GlyphMetrics, Metrics};
pub use palette::{ColorPalette, Usability};
pub use setting::Setting;
pub use strike::BitmapStrike;
pub use string::{LocalizedString, StringId};
pub use tag::{tag_from_bytes, tag_from_str_lossy, Tag};
pub use variation::{Instance, Variation};

/// Collection of various iterators over metadata contained in a font.
pub mod iter {
    pub use super::feature::{Features, WritingSystems};
    pub use super::font::Fonts;
    pub use super::palette::ColorPalettes;
    pub use super::strike::BitmapStrikes;
    pub use super::string::{Chars, LocalizedStrings};
    pub use super::variation::{Instances, Variations};
}

/// Proxies used to efficiently rematerialize metadata.
pub mod proxy {
    pub use super::charmap::CharmapProxy;
    pub use super::metrics::MetricsProxy;
    pub use super::strike::BitmapStrikesProxy;
    pub use super::variation::VariationsProxy;
}

use iter::*;
use proxy::BitmapStrikesProxy;

/// Glyph identifier.
pub type GlyphId = u16;

/// Normalized variation coordinate in 2.14 fixed point format.
pub type NormalizedCoord = i16;

impl<'a> FontRef<'a> {
    /// Returns the primary attributes for the font.
    pub fn attributes(&self) -> Attributes {
        Attributes::from_font(self)
    }

    /// Returns an iterator over the localized strings for the font.
    pub fn localized_strings(&self) -> LocalizedStrings<'a> {
        LocalizedStrings::from_font(self)
    }

    /// Returns an iterator over the variations for the font.
    pub fn variations(&self) -> Variations<'a> {
        Variations::from_font(self)
    }

    /// Returns an iterator over the named instances for the font.
    pub fn instances(&self) -> Instances<'a> {
        Instances::from_font(self)
    }

    /// Returns an iterator over writing systems supported by the font.
    pub fn writing_systems(&self) -> WritingSystems<'a> {
        WritingSystems::from_font(self)
    }

    /// Returns an iterator over the features supported by a font.
    pub fn features(&self) -> Features<'a> {
        Features::from_font(self)
    }

    /// Returns metrics for the font and the specified normalized variation
    /// coordinates.
    pub fn metrics(&self, coords: &'a [NormalizedCoord]) -> Metrics {
        Metrics::from_font(self, coords)
    }

    /// Returns glyph metrics for the font and the specified normalized
    /// variation coordinates.
    pub fn glyph_metrics(&self, coords: &'a [NormalizedCoord]) -> GlyphMetrics<'a> {
        GlyphMetrics::from_font(self, coords)
    }

    /// Returns the character map for the font.
    pub fn charmap(&self) -> Charmap<'a> {
        Charmap::from_font(self)
    }

    /// Returns an iterator over the color palettes for the font.
    pub fn color_palettes(&self) -> ColorPalettes<'a> {
        ColorPalettes::from_font(self)
    }

    /// Returns an iterator over the alpha bitmap strikes for the font.
    pub fn alpha_strikes(&self) -> BitmapStrikes<'a> {
        BitmapStrikesProxy::from_font(self).materialize_alpha(self)
    }

    /// Returns an iterator over the color bitmap strikes for the font.
    pub fn color_strikes(&self) -> BitmapStrikes<'a> {
        BitmapStrikesProxy::from_font(self).materialize_color(self)
    }

    /// Returns the table data for the specified tag.
    pub fn table(&self, tag: Tag) -> Option<&'a [u8]> {
        use internal::RawFont;
        let range = self.table_range(tag)?;
        self.data.get(range.0 as usize..range.1 as usize)
    }

    /// Returns the name for the specified glyph identifier. This is an internal
    /// function used for testing and stability is not guaranteed.
    #[doc(hidden)]
    pub fn glyph_name(&self, glyph_id: GlyphId) -> Option<&'a str> {
        use internal::head::Post;
        Post::from_font(self)?.name(glyph_id)
    }
}
