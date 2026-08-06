// OXITEXT MODIFICATION (oxitext 0.2.2): COOLJAPAN conformance pass, clippy
// `derivable_impls` -- the hand-written `impl Default for Content` replaced by `#[derive(Default)]` + `#[default]`.
// Machine-applied by `cargo clippy --fix`; no behaviour change.
// See ../../PROVENANCE.md.
/*!
Rendered glyph image.
*/

use super::Source;
use alloc::vec::Vec;
use zeno::Placement;

/// Content of a scaled glyph image.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Content {
    /// 8-bit alpha mask.
    #[default]
    Mask,
    /// 32-bit RGBA subpixel mask.
    SubpixelMask,
    /// 32-bit RGBA bitmap.
    Color,
}

/// Scaled glyph image.
#[derive(Clone, Default)]
pub struct Image {
    /// Source of the image.
    pub source: Source,
    /// Content of the image.
    pub content: Content,
    /// Offset and size of the image.
    pub placement: Placement,
    /// Raw image data.
    pub data: Vec<u8>,
}

impl Image {
    /// Creates a new empty scaled image.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets the image to a default state.
    pub fn clear(&mut self) {
        self.source = Source::default();
        self.content = Content::default();
        self.placement = Placement::default();
        self.data.clear();
    }
}
