//! Rasterization quality options.
//!
//! [`RasterOptions`] is the top-level configuration struct that controls
//! hinting, subpixel rendering, LCD filter selection, gamma correction, and
//! stem darkening.  A fluent [`RasterOptionsBuilder`] is provided for
//! ergonomic construction.
//!
//! # Examples
//!
//! ```rust
//! use oxitext_raster::options::{RasterOptions, SubpixelMode};
//!
//! let opts = RasterOptions::builder()
//!     .subpixel(SubpixelMode::Horizontal)
//!     .gamma(true)
//!     .build();
//! assert_eq!(opts.subpixel_mode, SubpixelMode::Horizontal);
//! assert!(opts.gamma_correction);
//! ```

/// Hinting mode for TrueType/OpenType outline instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HintingMode {
    /// No hinting — outlines are rendered as-is.
    #[default]
    None,
    /// Normal hinting — grid-fits stems and alignments.
    Normal,
    /// Full (bytecode) hinting — maximum grid alignment.
    Full,
}

/// Subpixel rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubpixelMode {
    /// Greyscale rendering — no subpixel decomposition.
    #[default]
    None,
    /// Horizontal RGB subpixel rendering (LCD panels).
    Horizontal,
}

/// LCD filter kernel for horizontal subpixel rendering.
///
/// The kernel is applied as a 1-D FIR convolution in the 3× horizontal
/// resolution space before decimation to the output RGB triplets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LcdFilterKernel {
    /// Box filter: uniform 3-tap `[1/3, 1/3, 1/3]`.
    Box,
    /// Triangle (tent) filter: 3-tap `[1/4, 1/2, 1/4]`.
    Triangle,
    /// FreeType 5-tap filter: `[1/9, 2/9, 3/9, 2/9, 1/9]` (default).
    #[default]
    FreeType5Tap,
}

/// Complete set of rasterization quality options.
///
/// Construct via [`RasterOptions::default`], [`RasterOptions::lcd`], or
/// the fluent [`RasterOptions::builder`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterOptions {
    /// TrueType/OpenType hinting mode.
    pub hinting_mode: HintingMode,
    /// Subpixel rendering mode.
    pub subpixel_mode: SubpixelMode,
    /// LCD filter kernel (relevant only when `subpixel_mode` is not `None`).
    pub lcd_filter: LcdFilterKernel,
    /// Apply sRGB gamma correction during blending.
    pub gamma_correction: bool,
    /// Apply FreeType-style stem darkening at small sizes.
    pub stem_darkening: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            hinting_mode: HintingMode::None,
            subpixel_mode: SubpixelMode::None,
            lcd_filter: LcdFilterKernel::FreeType5Tap,
            gamma_correction: true,
            stem_darkening: false,
        }
    }
}

impl RasterOptions {
    /// Return a [`RasterOptionsBuilder`] initialised from the default options.
    pub fn builder() -> RasterOptionsBuilder {
        RasterOptionsBuilder::default()
    }

    /// Convenience constructor that enables horizontal LCD subpixel rendering
    /// with all other fields set to their defaults.
    pub fn lcd() -> Self {
        Self {
            subpixel_mode: SubpixelMode::Horizontal,
            ..Self::default()
        }
    }
}

/// Fluent builder for [`RasterOptions`].
#[derive(Debug, Clone, Default)]
pub struct RasterOptionsBuilder(RasterOptions);

impl RasterOptionsBuilder {
    /// Set the hinting mode.
    pub fn hinting(mut self, mode: HintingMode) -> Self {
        self.0.hinting_mode = mode;
        self
    }

    /// Set the subpixel rendering mode.
    pub fn subpixel(mut self, mode: SubpixelMode) -> Self {
        self.0.subpixel_mode = mode;
        self
    }

    /// Set the LCD filter kernel.
    pub fn filter(mut self, kernel: LcdFilterKernel) -> Self {
        self.0.lcd_filter = kernel;
        self
    }

    /// Enable or disable gamma correction.
    pub fn gamma(mut self, enabled: bool) -> Self {
        self.0.gamma_correction = enabled;
        self
    }

    /// Enable or disable stem darkening.
    pub fn stem_darkening(mut self, enabled: bool) -> Self {
        self.0.stem_darkening = enabled;
        self
    }

    /// Build the final [`RasterOptions`].
    pub fn build(self) -> RasterOptions {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_build_without_panic() {
        let opts = RasterOptions::default();
        assert_eq!(opts.hinting_mode, HintingMode::None);
        assert_eq!(opts.subpixel_mode, SubpixelMode::None);
        assert_eq!(opts.lcd_filter, LcdFilterKernel::FreeType5Tap);
        assert!(opts.gamma_correction);
        assert!(!opts.stem_darkening);
    }

    #[test]
    fn builder_sets_all_fields() {
        let opts = RasterOptions::builder()
            .hinting(HintingMode::Full)
            .subpixel(SubpixelMode::Horizontal)
            .filter(LcdFilterKernel::Triangle)
            .gamma(false)
            .stem_darkening(true)
            .build();
        assert_eq!(opts.hinting_mode, HintingMode::Full);
        assert_eq!(opts.subpixel_mode, SubpixelMode::Horizontal);
        assert_eq!(opts.lcd_filter, LcdFilterKernel::Triangle);
        assert!(!opts.gamma_correction);
        assert!(opts.stem_darkening);
    }

    #[test]
    fn lcd_constructor_enables_horizontal() {
        let opts = RasterOptions::lcd();
        assert_eq!(opts.subpixel_mode, SubpixelMode::Horizontal);
        assert!(
            opts.gamma_correction,
            "LCD mode should keep gamma correction"
        );
    }
}
