//! `oxitext-core` — Core traits and value types for OxiText.
//!
//! This crate provides the shared data types used throughout the OxiText
//! pipeline: [`ShapedGlyph`], [`ShapedRun`], [`PositionedGlyph`], [`Bitmap`],
//! [`ColorBitmap`], [`LcdBitmap`], [`RenderOutput`],
//! [`LayoutConstraints`], [`TextStyle`], [`FlowDirection`], and [`OxiTextError`].
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::{string::String, sync::Arc, vec, vec::Vec};
#[cfg(feature = "std")]
use std::sync::Arc;

use smallvec::SmallVec;

/// A glyph produced by the shaper.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShapedGlyph {
    /// Glyph ID in the font.
    pub gid: u16,
    /// Horizontal advance in pixels (scaled by font size).
    pub x_advance: f32,
    /// Vertical advance (usually 0.0 for LTR text).
    pub y_advance: f32,
    /// Horizontal offset from the cursor position.
    pub x_offset: f32,
    /// Vertical offset from the baseline.
    pub y_offset: f32,
    /// Index into the source string (UTF-8 byte offset of cluster start).
    pub cluster: u32,
    /// `true` if this glyph represents whitespace (space, tab, newline).
    ///
    /// Layout engines use this to distinguish trimmable trailing whitespace
    /// and to compute expandable gaps for justified text.
    pub is_whitespace: bool,
    /// `true` if breaking a line *before* this glyph is unsafe because the
    /// glyph is part of a multi-glyph cluster (e.g. a ligature or a mark
    /// attached to a base glyph). Mirrors HarfBuzz's `unsafe_to_break` flag.
    pub unsafe_to_break: bool,
}

impl Default for ShapedGlyph {
    /// A `.notdef` glyph (GID 0) with zero advance and zero offsets.
    fn default() -> Self {
        Self {
            gid: 0,
            x_advance: 0.0,
            y_advance: 0.0,
            x_offset: 0.0,
            y_offset: 0.0,
            cluster: 0,
            is_whitespace: false,
            unsafe_to_break: false,
        }
    }
}

/// Font-wide vertical metrics needed to compute line height, in font design
/// units.
///
/// This is a deliberately minimal, font-library-agnostic mirror of the
/// ascender/descender/line-gap fields found in a font's `hhea`/`OS/2` tables.
/// Higher layers (e.g. the `oxitext` facade) translate their font library's
/// richer metrics type into this struct so the layout engine stays free of any
/// font-parser dependency.
///
/// Convert to pixels with `value * (font_size_px / units_per_em)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FontVerticalMetrics {
    /// Design units per em (typically 1000 for CFF, 2048 for TrueType).
    pub units_per_em: u16,
    /// Typographic ascender in design units (positive, above baseline).
    pub ascender: i16,
    /// Typographic descender in design units (negative, below baseline).
    pub descender: i16,
    /// Typographic line gap (extra leading between lines), in design units.
    pub line_gap: i16,
}

impl FontVerticalMetrics {
    /// Returns the pixel ascent (always positive) at `font_size_px`.
    pub fn ascent_px(&self, font_size_px: f32) -> f32 {
        if self.units_per_em == 0 {
            return font_size_px * 0.8;
        }
        self.ascender as f32 * font_size_px / self.units_per_em as f32
    }

    /// Returns the pixel descent depth (always positive) at `font_size_px`.
    pub fn descent_px(&self, font_size_px: f32) -> f32 {
        if self.units_per_em == 0 {
            return font_size_px * 0.2;
        }
        (-(self.descender as f32)) * font_size_px / self.units_per_em as f32
    }

    /// Returns the pixel line gap at `font_size_px`.
    pub fn line_gap_px(&self, font_size_px: f32) -> f32 {
        if self.units_per_em == 0 {
            return font_size_px * 0.4;
        }
        self.line_gap as f32 * font_size_px / self.units_per_em as f32
    }
}

/// Per-glyph metrics usable for layout without rasterising.
///
/// All values are in pixels (already scaled by the rendering font size). The
/// bearings follow the usual font conventions: `bearing_x` is the horizontal
/// distance from the pen origin to the left edge of the glyph bounding box,
/// and `bearing_y` is the vertical distance from the baseline to the top of
/// the bounding box (positive = above the baseline).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlyphMetrics {
    /// Horizontal distance from the pen origin to the left edge (signed).
    pub bearing_x: f32,
    /// Vertical distance from the baseline to the top edge (positive = up).
    pub bearing_y: f32,
    /// Horizontal advance in pixels.
    pub advance_x: f32,
    /// Vertical advance in pixels (usually `0.0` for horizontal text).
    pub advance_y: f32,
    /// Glyph bounding-box width in pixels.
    pub width: f32,
    /// Glyph bounding-box height in pixels.
    pub height: f32,
}

impl Default for GlyphMetrics {
    fn default() -> Self {
        Self {
            bearing_x: 0.0,
            bearing_y: 0.0,
            advance_x: 0.0,
            advance_y: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }
}

/// A group of [`ShapedGlyph`]s that together form a single user-perceived
/// grapheme cluster (e.g. a base letter plus combining marks, or an emoji
/// ZWJ sequence rendered as one glyph).
///
/// Clusters are the atomic unit for cursor movement, selection, and
/// line-breaking: a layout engine must never split text inside a cluster.
#[derive(Debug, Clone)]
pub struct GlyphCluster {
    /// The glyphs that make up this cluster, in logical order.
    pub glyphs: Vec<ShapedGlyph>,
    /// UTF-8 byte offset of the cluster start in the source string.
    pub source_start: u32,
    /// UTF-8 byte offset of the cluster end (exclusive) in the source string.
    pub source_end: u32,
}

impl GlyphCluster {
    /// Returns the total horizontal advance of all glyphs in the cluster.
    pub fn advance(&self) -> f32 {
        self.glyphs.iter().map(|g| g.x_advance).sum()
    }

    /// Returns `true` if the cluster contains no glyphs.
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }
}

/// A run of shaped glyphs sharing a single font face.
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// Glyphs in this run, in logical order.
    ///
    /// Uses [`SmallVec`] with an inline capacity of 8 to avoid heap allocation
    /// for the common case of short runs.
    pub glyphs: SmallVec<[ShapedGlyph; 8]>,
    /// Raw font bytes used to shape this run.
    pub font_data: Arc<[u8]>,
}

/// A glyph positioned on the layout canvas.
#[derive(Debug, Clone)]
pub struct PositionedGlyph {
    /// Glyph ID.
    pub gid: u16,
    /// Font data associated with this glyph.
    pub font_data: Arc<[u8]>,
    /// Position `(x, y)` in pixels from the top-left origin.
    pub pos: (f32, f32),
    /// Font size in pixels-per-em used to shape and rasterise this glyph.
    ///
    /// Carried per-glyph so that a single line may mix multiple sizes (e.g.
    /// superscripts, mixed-style runs) and the rasteriser knows the size for
    /// each glyph without re-deriving it from a shared style.
    pub font_size: f32,
    /// Horizontal advance in pixels (same unit as `pos`).
    ///
    /// Needed for hit-testing (cursor placement) and for determining a glyph's
    /// x-extent without referencing the original `ShapedRun` again.
    pub advance_x: f32,
    /// UTF-8 byte offset of this glyph's cluster in the source text.
    ///
    /// Mirrors [`ShapedGlyph::cluster`]. Carried here so that hit-testing,
    /// hanging-punctuation checks, and other post-layout passes can identify
    /// the source codepoint without walking the original `ShapedRun` list.
    pub cluster: u32,
}

/// A greyscale glyph bitmap.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bitmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel data, one byte per pixel (0 = transparent, 255 = fully opaque).
    pub pixels: Vec<u8>,
}

impl Bitmap {
    /// Returns `true` if the bitmap has zero area (no visible pixels), as is
    /// the case for whitespace glyphs.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.pixels.is_empty()
    }

    /// Invert the coverage values (`255 - x`) for use in inside/outside SDF generation.
    ///
    /// Coverage bitmaps from rasterizers use 255 = opaque, 0 = transparent.
    /// Some SDF algorithms expect the inverse convention where 0 = inside the
    /// glyph outline. This method produces a new bitmap with all values flipped.
    pub fn invert_coverage(&self) -> Self {
        Bitmap {
            width: self.width,
            height: self.height,
            pixels: self.pixels.iter().map(|&v| 255 - v).collect(),
        }
    }

    /// Return a copy with pixels below the threshold set to 0, above (or equal) to 255.
    ///
    /// Useful for binarizing a greyscale coverage map before Euclidean Distance
    /// Transform (EDT) so that only fully-inside and fully-outside pixels are
    /// distinguished.
    pub fn threshold(&self, threshold: u8) -> Self {
        Bitmap {
            width: self.width,
            height: self.height,
            pixels: self
                .pixels
                .iter()
                .map(|&v| if v >= threshold { 255 } else { 0 })
                .collect(),
        }
    }

    /// Return a cropped sub-bitmap starting at pixel `(x, y)` with the given
    /// `width` and `height`. Out-of-bounds source regions are filled with 0.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Self {
        let mut pixels = vec![0u8; (width * height) as usize];
        for row in 0..height {
            for col in 0..width {
                let src_x = x + col;
                let src_y = y + row;
                if src_x < self.width && src_y < self.height {
                    let src_idx = (src_y * self.width + src_x) as usize;
                    let dst_idx = (row * width + col) as usize;
                    pixels[dst_idx] = self.pixels[src_idx];
                }
            }
        }
        Bitmap {
            width,
            height,
            pixels,
        }
    }

    /// Return the minimum bounding box of non-zero pixels, useful for tight
    /// SDF tile sizing and atlas packing.
    ///
    /// Returns `(x_min, y_min, x_max, y_max)` in pixel coordinates, or `None`
    /// if the bitmap contains no non-zero pixels (e.g. a space glyph).
    pub fn tight_bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let mut x_min = self.width;
        let mut y_min = self.height;
        let mut x_max = 0u32;
        let mut y_max = 0u32;

        for row in 0..self.height {
            for col in 0..self.width {
                if self.pixels[(row * self.width + col) as usize] > 0 {
                    x_min = x_min.min(col);
                    y_min = y_min.min(row);
                    x_max = x_max.max(col);
                    y_max = y_max.max(row);
                }
            }
        }

        if x_min > x_max {
            None
        } else {
            Some((x_min, y_min, x_max, y_max))
        }
    }
}

/// An RGBA color glyph bitmap.
///
/// Produced by color-font rendering (COLR/CPAL, CBDT, sbix, SVG). Pixels are
/// stored in row-major RGBA order, four bytes per pixel.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColorBitmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel data in RGBA order: `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

impl ColorBitmap {
    /// Returns `true` if the bitmap has zero area.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.rgba.is_empty()
    }
}

/// An LCD subpixel bitmap.
///
/// Stores three bytes per pixel (R, G, B) corresponding to the physical
/// sub-pixel layout of an LCD screen. LCD rendering allows individual
/// sub-pixel addressing for smoother horizontal antialiasing at small
/// sizes on colour displays.
///
/// The buffer length must equal `width * height * 3`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LcdBitmap {
    /// Width in pixels (each pixel contains 3 sub-pixel bytes).
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Sub-pixel data in RGB order: `width * height * 3` bytes.
    pub rgb: Vec<u8>,
}

impl LcdBitmap {
    /// Constructs a new [`LcdBitmap`] from its components.
    ///
    /// # Panics (debug only)
    ///
    /// In debug builds a debug assertion fires if `rgb.len()` does not equal
    /// `width * height * 3`, catching accidental buffer-size mismatches early.
    pub fn new(width: u32, height: u32, rgb: Vec<u8>) -> Self {
        debug_assert_eq!(
            rgb.len(),
            (width as usize) * (height as usize) * 3,
            "LcdBitmap: rgb buffer length must equal width * height * 3"
        );
        Self { width, height, rgb }
    }

    /// Returns `true` if the bitmap has zero area.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.rgb.is_empty()
    }
}

/// Unified per-glyph render output.
///
/// Lets a rendering pipeline return greyscale, color, SDF, LCD subpixel, or
/// multi-channel SDF output through a single channel so callers can handle a
/// mixed set of glyphs uniformly.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RenderOutput {
    /// A greyscale coverage bitmap.
    Greyscale(Bitmap),
    /// An RGBA color bitmap (color fonts).
    Color(ColorBitmap),
    /// A single-channel signed-distance-field tile (`width * height` bytes).
    Sdf {
        /// Tile width in pixels.
        width: u32,
        /// Tile height in pixels.
        height: u32,
        /// SDF bytes (`< 128` outside, `≈ 128` outline, `> 128` inside).
        data: Vec<u8>,
    },
    /// An LCD subpixel bitmap (three bytes per pixel: R, G, B channels).
    ///
    /// Used for ClearType / FreeType LCD rendering to achieve sub-pixel
    /// horizontal precision on colour LCD displays.
    Lcd(LcdBitmap),
    /// A multi-channel signed-distance-field tile.
    ///
    /// MSDF encodes the distance field across three independent colour channels
    /// to resolve corner artefacts that appear in single-channel SDF at large
    /// magnifications. The data layout is `width * height * 3` bytes (RGB).
    Msdf {
        /// Tile width in pixels.
        width: u32,
        /// Tile height in pixels.
        height: u32,
        /// MSDF bytes in RGB order: `width * height * 3` bytes.
        data: Vec<u8>,
    },
}

/// Layout constraints for the layouter.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayoutConstraints {
    /// Maximum line width in pixels (0.0 = no wrap).
    pub max_width: f32,
    /// Font size in points.
    pub font_size: f32,
}

impl Default for LayoutConstraints {
    fn default() -> Self {
        Self {
            max_width: 800.0,
            font_size: 16.0,
        }
    }
}

/// Text flow direction for a rendering run.
///
/// Governs how the layout engine advances the cursor between glyphs and lines.
/// Horizontal is the default (left-to-right or bidi-resolved RTL within lines).
/// Vertical enables top-to-bottom CJK flow as per UAX #50.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FlowDirection {
    /// Standard horizontal text (LTR/RTL decided by bidi algorithm).
    #[default]
    Horizontal,
    /// Vertical text, advancing top-to-bottom (used for CJK vertical layout).
    Vertical,
}

/// Horizontal text alignment within the layout's line box.
///
/// Per CSS Text Module Level 3 `text-align`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAlignment {
    /// Align lines to the start (left edge for LTR, right edge for RTL).
    #[default]
    Left,
    /// Align lines to the right edge.
    Right,
    /// Center lines within the available width.
    Center,
    /// Stretch lines to fill the available width by expanding inter-word gaps
    /// (the last line of a paragraph is not justified).
    Justify,
}

/// CSS Writing Modes Level 4 `writing-mode`.
///
/// Determines the block flow direction and inline base direction. This is a
/// richer companion to [`FlowDirection`]: `HorizontalTb` corresponds to
/// [`FlowDirection::Horizontal`], while the two vertical modes map to
/// [`FlowDirection::Vertical`] with differing block progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WritingMode {
    /// Horizontal lines stacked top-to-bottom (Latin, Cyrillic, etc.).
    #[default]
    HorizontalTb,
    /// Vertical lines progressing right-to-left (traditional CJK).
    VerticalRl,
    /// Vertical lines progressing left-to-right (Mongolian, some CJK).
    VerticalLr,
}

impl WritingMode {
    /// Returns the [`FlowDirection`] implied by this writing mode.
    pub fn flow_direction(self) -> FlowDirection {
        match self {
            WritingMode::HorizontalTb => FlowDirection::Horizontal,
            WritingMode::VerticalRl | WritingMode::VerticalLr => FlowDirection::Vertical,
        }
    }

    /// Returns `true` if this writing mode lays text out vertically.
    pub fn is_vertical(self) -> bool {
        !matches!(self, WritingMode::HorizontalTb)
    }
}

/// Line spacing configuration.
///
/// The effective line height is computed as
/// `font_ascent + font_descent + line_gap` (the font's natural line height)
/// multiplied by `line_height_multiplier`, plus `leading` extra pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineSpacing {
    /// Extra leading added between baselines, in pixels.
    pub leading: f32,
    /// Multiplier applied to the natural font line height (1.0 = single).
    pub line_height_multiplier: f32,
}

impl Default for LineSpacing {
    fn default() -> Self {
        Self {
            leading: 0.0,
            line_height_multiplier: 1.0,
        }
    }
}

impl LineSpacing {
    /// Computes the effective line height in pixels from a natural line height.
    pub fn resolve(&self, natural_line_height: f32) -> f32 {
        natural_line_height * self.line_height_multiplier + self.leading
    }
}

/// An sRGB color with straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rgba8 {
    /// Red channel (0–255).
    pub r: u8,
    /// Green channel (0–255).
    pub g: u8,
    /// Blue channel (0–255).
    pub b: u8,
    /// Alpha channel (0 = transparent, 255 = opaque).
    pub a: u8,
}

impl Rgba8 {
    /// Opaque black.
    pub const BLACK: Rgba8 = Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// Fully transparent.
    pub const TRANSPARENT: Rgba8 = Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Constructs a new color from components.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Rgba8 {
    fn default() -> Self {
        Rgba8::BLACK
    }
}

/// A single text decoration line (underline, overline, or strikethrough).
///
/// Position and thickness are in pixels relative to the text baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecorationLine {
    /// Distance from the baseline to the decoration line, in pixels. By
    /// convention positive values are above the baseline (overline,
    /// strikethrough) and negative values below (underline).
    pub position: f32,
    /// Stroke thickness in pixels.
    pub thickness: f32,
    /// Decoration color.
    pub color: Rgba8,
}

/// A text decoration style applied to a run of text.
///
/// Describes the visual decoration (underline, overline, or strikethrough) and
/// its rendering parameters. Used with `LayoutOptions::decoration` to
/// produce [`DecorationRect`]s from a layout pass.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextDecoration {
    /// Underline drawn below the text baseline.
    Underline {
        /// Color of the underline (RGBA).
        color: Rgba8,
        /// Thickness in pixels (default: 1.0).
        thickness: f32,
        /// Vertical offset from baseline in pixels (positive = downward).
        offset: f32,
    },
    /// Overline drawn above the ascender line.
    Overline {
        /// Color of the overline (RGBA).
        color: Rgba8,
        /// Thickness in pixels.
        thickness: f32,
        /// Vertical offset from the top of the ascender (positive = upward
        /// from the ascender line).
        offset: f32,
    },
    /// Strikethrough drawn through the middle of the text (at x-height
    /// midpoint).
    Strikethrough {
        /// Color of the strikethrough (RGBA).
        color: Rgba8,
        /// Thickness in pixels.
        thickness: f32,
    },
}

/// A positioned decoration rectangle ready to be composited onto the output
/// canvas.
///
/// Produced by the layout engine when `LayoutOptions::decoration` is set.
/// The caller is responsible for painting the rectangle (e.g. by calling
/// `RenderResult::composite_to_rgba` which applies decorations
/// automatically).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecorationRect {
    /// Left edge in canvas pixels.
    pub x: f32,
    /// Top edge in canvas pixels.
    pub y: f32,
    /// Width in canvas pixels.
    pub width: f32,
    /// Height in canvas pixels (equals the decoration thickness).
    pub height: f32,
    /// Color of the decoration.
    pub color: Rgba8,
}

/// Text decorations applied to a run: underline, overline, strikethrough.
///
/// Each field is `Some` when the corresponding decoration is enabled. Default
/// is no decorations.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Decoration {
    /// Underline (below the baseline), if any.
    pub underline: Option<DecorationLine>,
    /// Overline (above the text), if any.
    pub overline: Option<DecorationLine>,
    /// Strikethrough (through the text), if any.
    pub strikethrough: Option<DecorationLine>,
}

impl Decoration {
    /// Returns `true` if any decoration line is enabled.
    pub fn any(&self) -> bool {
        self.underline.is_some() || self.overline.is_some() || self.strikethrough.is_some()
    }
}

/// Text rendering style.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextStyle {
    /// Font size in points.
    pub font_size: f32,
    /// Maximum line width in pixels (0.0 = no wrap).
    pub max_width: f32,
    /// Text flow direction (horizontal or vertical).
    pub flow_direction: FlowDirection,
    /// Horizontal alignment of laid-out lines.
    pub alignment: TextAlignment,
    /// Line spacing configuration.
    pub line_spacing: LineSpacing,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            max_width: 800.0,
            flow_direction: FlowDirection::Horizontal,
            alignment: TextAlignment::Left,
            line_spacing: LineSpacing::default(),
        }
    }
}

impl TextStyle {
    /// Returns a copy of this style with the given alignment.
    pub fn with_alignment(mut self, alignment: TextAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Returns a copy of this style with the given font size (pixels-per-em).
    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// Returns a copy of this style with the given maximum line width.
    pub fn with_max_width(mut self, max_width: f32) -> Self {
        self.max_width = max_width;
        self
    }

    /// Returns a copy of this style with the given flow direction.
    pub fn with_flow_direction(mut self, flow_direction: FlowDirection) -> Self {
        self.flow_direction = flow_direction;
        self
    }
}

/// Paragraph-level layout style.
///
/// Governs alignment, indentation, vertical spacing around the paragraph, and
/// base direction. Per CSS Text / Writing Modes.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParagraphStyle {
    /// Horizontal alignment of lines within the paragraph.
    pub alignment: TextAlignment,
    /// First-line indent in pixels.
    pub indent: f32,
    /// Vertical space before the paragraph, in pixels.
    pub spacing_before: f32,
    /// Vertical space after the paragraph, in pixels.
    pub spacing_after: f32,
    /// Base flow direction for the paragraph.
    pub direction: FlowDirection,
    /// Line spacing within the paragraph.
    pub line_spacing: LineSpacing,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            alignment: TextAlignment::Left,
            indent: 0.0,
            spacing_before: 0.0,
            spacing_after: 0.0,
            direction: FlowDirection::Horizontal,
            line_spacing: LineSpacing::default(),
        }
    }
}

/// A styled span of text within a paragraph.
///
/// Pairs a text slice with the font bytes to shape it and a [`TextStyle`].
/// Used by multi-style ("rich text") layout where a single paragraph mixes
/// fonts, sizes, and decorations.
#[derive(Debug, Clone)]
pub struct TextRun {
    /// The text content of this run.
    pub text: String,
    /// Font bytes used to shape and rasterise this run.
    pub font_data: Arc<[u8]>,
    /// Rendering style for this run.
    pub style: TextStyle,
    /// Optional text decorations for this run.
    pub decoration: Decoration,
}

/// An inline object (image, custom widget) that can be positioned inline with text.
/// The layout engine treats it as a glyph with known advance and baseline offset.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineObject {
    /// Unique identifier for this object (caller-defined, used for lookup after layout).
    pub id: u64,
    /// Width in pixels.
    pub width: f32,
    /// Height in pixels.
    pub height: f32,
    /// Offset from the text baseline in pixels (positive = above baseline, for typical images).
    pub baseline_offset: f32,
    /// Horizontal advance (usually == width, but may differ for glyph-adjacent images).
    pub advance: f32,
}

/// A positioned inline object from a layout pass.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedInlineObject {
    /// The inline object descriptor.
    pub object: InlineObject,
    /// X position in canvas pixels.
    pub x: f32,
    /// Y position in canvas pixels (of the baseline).
    pub y: f32,
    /// Line index (0-based) this object is placed on.
    pub line: usize,
}

/// Vertical text positioning for subscript/superscript effects.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum VerticalPosition {
    /// Normal baseline.
    #[default]
    Normal,
    /// Superscript: smaller text raised above the baseline.
    Superscript {
        /// Font size ratio (e.g. 0.6 for 60% of base size).
        size_ratio: f32,
        /// Baseline rise in pixels (positive = upward).
        baseline_rise: f32,
    },
    /// Subscript: smaller text lowered below the baseline.
    Subscript {
        /// Font size ratio (e.g. 0.6 for 60% of base size).
        size_ratio: f32,
        /// Baseline drop in pixels (positive = downward).
        baseline_drop: f32,
    },
}

impl VerticalPosition {
    /// Compute the actual font size for this position given a base size.
    pub fn effective_size(&self, base_px: f32) -> f32 {
        match self {
            Self::Normal => base_px,
            Self::Superscript { size_ratio, .. } => base_px * size_ratio,
            Self::Subscript { size_ratio, .. } => base_px * size_ratio,
        }
    }

    /// Compute the Y baseline adjustment in pixels (positive = upward).
    pub fn baseline_adjustment(&self, _base_px: f32) -> f32 {
        match self {
            Self::Normal => 0.0,
            Self::Superscript { baseline_rise, .. } => *baseline_rise,
            Self::Subscript { baseline_drop, .. } => -*baseline_drop,
        }
    }
}

/// Errors returned by the OxiText pipeline.
#[derive(Debug)]
pub enum OxiTextError {
    /// An error occurred during glyph shaping.
    Shaping(String),
    /// An error occurred during layout computation.
    Layout(String),
    /// An error occurred during glyph rasterization.
    Raster(String),
    /// No usable font was found.
    FontNotFound,
    /// The supplied font data is corrupt or uses an unsupported format.
    InvalidFont,
    /// A miscellaneous error not covered by a more specific variant.
    Other(String),
}

impl core::fmt::Display for OxiTextError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            OxiTextError::Shaping(s) => write!(f, "shaping error: {s}"),
            OxiTextError::Layout(s) => write!(f, "layout error: {s}"),
            OxiTextError::Raster(s) => write!(f, "raster error: {s}"),
            OxiTextError::FontNotFound => write!(f, "font not found"),
            OxiTextError::InvalidFont => write!(f, "invalid font"),
            OxiTextError::Other(s) => write!(f, "text error: {s}"),
        }
    }
}

impl core::error::Error for OxiTextError {}

impl RenderOutput {
    /// Extracts the greyscale [`Bitmap`] from a [`RenderOutput::Greyscale`] variant,
    /// returning `None` for all other variants.
    pub fn into_bitmap(self) -> Option<Bitmap> {
        match self {
            RenderOutput::Greyscale(b) => Some(b),
            _ => None,
        }
    }
}

impl From<RenderOutput> for Option<Bitmap> {
    /// Converts a [`RenderOutput`] into `Some(Bitmap)` for the greyscale variant,
    /// or `None` for all other variants.
    fn from(output: RenderOutput) -> Self {
        output.into_bitmap()
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn layout_constraints_default_values() {
        let c = LayoutConstraints::default();
        assert_eq!(c.max_width, 800.0);
        assert_eq!(c.font_size, 16.0);
    }

    #[test]
    fn text_style_default_values() {
        let s = TextStyle::default();
        assert_eq!(s.font_size, 16.0);
        assert_eq!(s.max_width, 800.0);
        assert_eq!(s.flow_direction, FlowDirection::Horizontal);
        assert_eq!(s.alignment, TextAlignment::Left);
        assert_eq!(s.line_spacing.line_height_multiplier, 1.0);
    }

    #[test]
    fn text_style_builders() {
        let s = TextStyle::default()
            .with_alignment(TextAlignment::Center)
            .with_font_size(24.0)
            .with_max_width(400.0);
        assert_eq!(s.alignment, TextAlignment::Center);
        assert_eq!(s.font_size, 24.0);
        assert_eq!(s.max_width, 400.0);
    }

    #[test]
    fn shaped_glyph_default_is_notdef() {
        let g = ShapedGlyph::default();
        assert_eq!(g.gid, 0);
        assert_eq!(g.x_advance, 0.0);
        assert!(!g.is_whitespace);
        assert!(!g.unsafe_to_break);
    }

    #[test]
    fn glyph_metrics_default_is_zero() {
        let m = GlyphMetrics::default();
        assert_eq!(m.advance_x, 0.0);
        assert_eq!(m.width, 0.0);
    }

    #[test]
    fn writing_mode_flow_direction_mapping() {
        assert_eq!(
            WritingMode::HorizontalTb.flow_direction(),
            FlowDirection::Horizontal
        );
        assert_eq!(
            WritingMode::VerticalRl.flow_direction(),
            FlowDirection::Vertical
        );
        assert_eq!(
            WritingMode::VerticalLr.flow_direction(),
            FlowDirection::Vertical
        );
        assert!(!WritingMode::HorizontalTb.is_vertical());
        assert!(WritingMode::VerticalRl.is_vertical());
    }

    #[test]
    fn line_spacing_resolve() {
        let ls = LineSpacing {
            leading: 2.0,
            line_height_multiplier: 1.5,
        };
        // natural 20 → 20*1.5 + 2 = 32
        assert!((ls.resolve(20.0) - 32.0).abs() < f32::EPSILON);
        let def = LineSpacing::default();
        assert!((def.resolve(20.0) - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn decoration_any_flag() {
        let none = Decoration::default();
        assert!(!none.any());
        let under = Decoration {
            underline: Some(DecorationLine {
                position: -2.0,
                thickness: 1.0,
                color: Rgba8::BLACK,
            }),
            ..Default::default()
        };
        assert!(under.any());
    }

    #[test]
    fn glyph_cluster_advance_and_empty() {
        let empty = GlyphCluster {
            glyphs: vec![],
            source_start: 0,
            source_end: 0,
        };
        assert!(empty.is_empty());
        assert_eq!(empty.advance(), 0.0);

        let cluster = GlyphCluster {
            glyphs: vec![
                ShapedGlyph {
                    x_advance: 10.0,
                    ..Default::default()
                },
                ShapedGlyph {
                    x_advance: 5.0,
                    ..Default::default()
                },
            ],
            source_start: 0,
            source_end: 3,
        };
        assert!(!cluster.is_empty());
        assert!((cluster.advance() - 15.0).abs() < f32::EPSILON);
    }

    #[test]
    fn bitmap_and_color_bitmap_empty() {
        let bm = Bitmap {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        assert!(bm.is_empty());
        let cbm = ColorBitmap {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
        };
        assert!(!cbm.is_empty());
    }

    #[test]
    fn render_output_variants_construct() {
        let g = RenderOutput::Greyscale(Bitmap {
            width: 1,
            height: 1,
            pixels: vec![255],
        });
        let c = RenderOutput::Color(ColorBitmap {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        });
        let s = RenderOutput::Sdf {
            width: 1,
            height: 1,
            data: vec![128],
        };
        let lcd = RenderOutput::Lcd(LcdBitmap::new(1, 1, vec![255, 0, 0]));
        let msdf = RenderOutput::Msdf {
            width: 1,
            height: 1,
            data: vec![100, 128, 200],
        };
        // Pattern-match to exercise each arm.
        assert!(matches!(g, RenderOutput::Greyscale(_)));
        assert!(matches!(c, RenderOutput::Color(_)));
        assert!(matches!(s, RenderOutput::Sdf { .. }));
        assert!(matches!(lcd, RenderOutput::Lcd(_)));
        assert!(matches!(msdf, RenderOutput::Msdf { .. }));
    }

    #[test]
    fn lcd_bitmap_new_constructor() {
        let bm = LcdBitmap::new(4, 2, vec![0u8; 4 * 2 * 3]);
        assert_eq!(bm.width, 4);
        assert_eq!(bm.height, 2);
        assert_eq!(bm.rgb.len(), 24);
        assert!(!bm.is_empty());
    }

    #[test]
    fn lcd_bitmap_is_empty() {
        let empty_w = LcdBitmap {
            width: 0,
            height: 1,
            rgb: vec![],
        };
        assert!(empty_w.is_empty());
        let empty_h = LcdBitmap {
            width: 1,
            height: 0,
            rgb: vec![],
        };
        assert!(empty_h.is_empty());
        let empty_buf = LcdBitmap {
            width: 1,
            height: 1,
            rgb: vec![],
        };
        assert!(empty_buf.is_empty());
    }

    #[test]
    fn msdf_variant_fields() {
        let msdf = RenderOutput::Msdf {
            width: 8,
            height: 8,
            data: vec![0u8; 8 * 8 * 3],
        };
        if let RenderOutput::Msdf {
            width,
            height,
            data,
        } = &msdf
        {
            assert_eq!(*width, 8);
            assert_eq!(*height, 8);
            assert_eq!(data.len(), 192);
        } else {
            panic!("expected Msdf variant");
        }
    }

    #[test]
    fn positioned_glyph_carries_font_size() {
        let pg = PositionedGlyph {
            gid: 5,
            font_data: Arc::from(&[][..]),
            pos: (1.0, 2.0),
            font_size: 18.0,
            advance_x: 12.0,
            cluster: 0,
        };
        assert_eq!(pg.font_size, 18.0);
    }

    #[test]
    fn text_run_construction() {
        let run = TextRun {
            text: "hi".to_string(),
            font_data: Arc::from(&[][..]),
            style: TextStyle::default(),
            decoration: Decoration::default(),
        };
        assert_eq!(run.text, "hi");
        assert!(!run.decoration.any());
    }

    #[test]
    fn flow_direction_is_hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(FlowDirection::Horizontal);
        set.insert(FlowDirection::Vertical);
        set.insert(FlowDirection::Horizontal);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn text_alignment_is_hashable() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(TextAlignment::Left, 1);
        map.insert(TextAlignment::Center, 2);
        assert_eq!(map.get(&TextAlignment::Left), Some(&1));
    }

    #[test]
    fn oxitext_error_display_all_variants() {
        assert_eq!(
            OxiTextError::Shaping("x".into()).to_string(),
            "shaping error: x"
        );
        assert_eq!(
            OxiTextError::Layout("x".into()).to_string(),
            "layout error: x"
        );
        assert_eq!(
            OxiTextError::Raster("x".into()).to_string(),
            "raster error: x"
        );
        assert_eq!(OxiTextError::FontNotFound.to_string(), "font not found");
        assert_eq!(OxiTextError::InvalidFont.to_string(), "invalid font");
        assert_eq!(OxiTextError::Other("x".into()).to_string(), "text error: x");
    }

    // ── Test 1a: FlowDirection property tests ────────────────────────────────

    #[test]
    fn test_flow_direction_equality() {
        assert_eq!(FlowDirection::Horizontal, FlowDirection::Horizontal);
        assert_ne!(FlowDirection::Horizontal, FlowDirection::Vertical);
    }

    #[test]
    fn test_flow_direction_clone() {
        let a = FlowDirection::Vertical;
        #[allow(clippy::clone_on_copy)]
        let b = Clone::clone(&a);
        assert_eq!(a, b);
    }

    #[test]
    fn test_flow_direction_debug() {
        let s = format!("{:?}", FlowDirection::Horizontal);
        assert!(s.contains("Horizontal"));
    }

    #[test]
    fn test_text_alignment_ordering() {
        // TextAlignment should support equality
        assert_eq!(TextAlignment::Left, TextAlignment::Left);
        assert_ne!(TextAlignment::Left, TextAlignment::Right);
    }

    // ── Test 1b: ShapedGlyph with negative offsets (combining marks) ─────────

    #[test]
    fn test_shaped_glyph_negative_offsets() {
        // Combining marks (diacritics) have negative y_offset to position above the base
        let g = ShapedGlyph {
            gid: 0x301,     // combining acute accent
            x_advance: 0.0, // zero-width
            y_advance: 0.0,
            x_offset: -2.5, // shifted left onto the base glyph
            y_offset: -8.0, // shifted up above baseline
            cluster: 0,
            is_whitespace: false,
            unsafe_to_break: true, // unsafe to break with base glyph
        };
        assert!(g.x_offset < 0.0);
        assert!(g.y_offset < 0.0);
        assert!(g.unsafe_to_break);
        assert_eq!(g.x_advance, 0.0);
    }

    #[test]
    fn test_shaped_glyph_default_is_notdef() {
        let g = ShapedGlyph::default();
        assert_eq!(g.gid, 0);
        assert_eq!(g.x_advance, 0.0);
        assert!(!g.unsafe_to_break);
    }

    // ── Test 1c: OxiTextError variants ───────────────────────────────────────

    #[test]
    fn test_error_display() {
        let e = OxiTextError::FontNotFound;
        let s = format!("{e}");
        assert!(!s.is_empty());
    }

    #[test]
    fn test_error_invalid_font() {
        let e = OxiTextError::InvalidFont;
        assert_ne!(format!("{e}"), format!("{}", OxiTextError::FontNotFound));
    }

    #[test]
    fn types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ShapedGlyph>();
        assert_send_sync::<ShapedRun>();
        assert_send_sync::<PositionedGlyph>();
        assert_send_sync::<Bitmap>();
        assert_send_sync::<ColorBitmap>();
        assert_send_sync::<LcdBitmap>();
        assert_send_sync::<RenderOutput>();
        assert_send_sync::<TextStyle>();
        assert_send_sync::<ParagraphStyle>();
        assert_send_sync::<TextRun>();
        assert_send_sync::<GlyphCluster>();
        assert_send_sync::<GlyphMetrics>();
    }

    #[test]
    fn render_output_into_bitmap_greyscale() {
        let bm = Bitmap {
            width: 4,
            height: 4,
            pixels: vec![255u8; 16],
        };
        let out = RenderOutput::Greyscale(bm.clone());
        let extracted: Option<Bitmap> = out.into();
        assert!(extracted.is_some());
        let extracted = extracted.expect("greyscale should yield Some(Bitmap)");
        assert_eq!(extracted.width, 4);
        assert_eq!(extracted.pixels.len(), 16);
    }

    #[test]
    fn render_output_into_bitmap_non_greyscale_is_none() {
        let out = RenderOutput::Sdf {
            width: 4,
            height: 4,
            data: vec![128u8; 16],
        };
        let extracted: Option<Bitmap> = out.into();
        assert!(extracted.is_none());

        let out2 = RenderOutput::Msdf {
            width: 4,
            height: 4,
            data: vec![100u8; 48],
        };
        let extracted2: Option<Bitmap> = out2.into();
        assert!(extracted2.is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_bitmap() {
        let bm = Bitmap {
            width: 2,
            height: 2,
            pixels: vec![0, 128, 200, 255],
        };
        let json = serde_json::to_string(&bm).expect("serialize Bitmap");
        let back: Bitmap = serde_json::from_str(&json).expect("deserialize Bitmap");
        assert_eq!(back.width, bm.width);
        assert_eq!(back.pixels, bm.pixels);
    }

    #[test]
    fn test_decoration_rect_fields() {
        let r = DecorationRect {
            x: 1.0,
            y: 2.0,
            width: 10.0,
            height: 1.5,
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
        };
        assert_eq!(r.width, 10.0);
        assert_eq!(r.height, 1.5);
        assert_eq!(r.color.a, 255);
    }

    #[test]
    fn test_text_decoration_variants() {
        let under = TextDecoration::Underline {
            color: Rgba8::BLACK,
            thickness: 1.0,
            offset: 2.0,
        };
        let over = TextDecoration::Overline {
            color: Rgba8::BLACK,
            thickness: 1.0,
            offset: 0.0,
        };
        let strike = TextDecoration::Strikethrough {
            color: Rgba8::BLACK,
            thickness: 1.5,
        };
        assert_ne!(under, over);
        assert_ne!(under, strike);
        // TextDecoration is Copy
        let _copy = under;
        let _copy2 = over;
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_text_style() {
        let style = TextStyle {
            font_size: 24.0,
            max_width: 600.0,
            flow_direction: FlowDirection::Vertical,
            alignment: TextAlignment::Center,
            line_spacing: LineSpacing {
                leading: 2.0,
                line_height_multiplier: 1.5,
            },
        };
        let json = serde_json::to_string(&style).expect("serialize TextStyle");
        let back: TextStyle = serde_json::from_str(&json).expect("deserialize TextStyle");
        assert_eq!(back.font_size, 24.0);
        assert_eq!(back.alignment, TextAlignment::Center);
        assert_eq!(back.flow_direction, FlowDirection::Vertical);
    }

    // ── Bitmap SDF alignment helpers ─────────────────────────────────────────

    #[test]
    fn test_bitmap_invert_coverage() {
        let b = Bitmap {
            width: 2,
            height: 1,
            pixels: vec![0u8, 255],
        };
        let inv = b.invert_coverage();
        assert_eq!(inv.pixels[0], 255);
        assert_eq!(inv.pixels[1], 0);
    }

    #[test]
    fn test_bitmap_threshold() {
        let b = Bitmap {
            width: 3,
            height: 1,
            pixels: vec![64u8, 128, 200],
        };
        let t = b.threshold(128);
        assert_eq!(t.pixels[0], 0);
        assert_eq!(t.pixels[1], 255);
        assert_eq!(t.pixels[2], 255);
    }

    #[test]
    fn test_bitmap_tight_bounds_all_zero_returns_none() {
        let b = Bitmap {
            width: 4,
            height: 4,
            pixels: vec![0u8; 16],
        };
        assert!(b.tight_bounds().is_none());
    }

    #[test]
    fn test_bitmap_tight_bounds_single_pixel() {
        let mut pixels = vec![0u8; 16];
        pixels[4 * 2 + 1] = 255; // row 2, col 1
        let b = Bitmap {
            width: 4,
            height: 4,
            pixels,
        };
        let bounds = b.tight_bounds().expect("should find pixel");
        assert_eq!(bounds, (1, 2, 1, 2));
    }

    #[test]
    fn test_bitmap_crop() {
        let pixels = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let b = Bitmap {
            width: 4,
            height: 4,
            pixels,
        };
        let cropped = b.crop(1, 1, 2, 2);
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(cropped.pixels, vec![6u8, 7, 10, 11]);
    }

    #[test]
    fn test_bitmap_invert_is_involution() {
        let b = Bitmap {
            width: 3,
            height: 1,
            pixels: vec![10u8, 128, 200],
        };
        let double_inv = b.invert_coverage().invert_coverage();
        assert_eq!(double_inv.pixels, b.pixels);
    }

    #[test]
    fn test_bitmap_crop_out_of_bounds_fills_zero() {
        let b = Bitmap {
            width: 2,
            height: 2,
            pixels: vec![1u8, 2, 3, 4],
        };
        // Crop starting beyond the bitmap width; all pixels should be 0
        let cropped = b.crop(5, 5, 3, 3);
        assert_eq!(cropped.pixels, vec![0u8; 9]);
    }

    #[test]
    fn test_std_feature_enabled_by_default() {
        // This test verifies the feature flag logic compiles correctly.
        // In a no_std build (--no-default-features), this test wouldn't run.
        #[cfg(feature = "std")]
        {
            // std is enabled — we can use core::error::Error
            let err: &dyn core::error::Error = &OxiTextError::InvalidFont;
            let _ = err.to_string();
        }
    }

    #[test]
    fn test_vertical_position_effective_size() {
        let vp = VerticalPosition::Superscript {
            size_ratio: 0.6,
            baseline_rise: 4.0,
        };
        assert!((vp.effective_size(16.0) - 9.6).abs() < 0.001);
    }

    #[test]
    fn test_vertical_position_baseline_adjustment() {
        let sub = VerticalPosition::Subscript {
            size_ratio: 0.6,
            baseline_drop: 3.0,
        };
        assert_eq!(sub.baseline_adjustment(16.0), -3.0);
        let norm = VerticalPosition::Normal;
        assert_eq!(norm.baseline_adjustment(16.0), 0.0);
    }
}
