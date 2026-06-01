//! Glyph outline (bezier path) extraction via ttf-parser.
//!
//! Provides [`extract_glyph_outline`] which reads a glyph's outline from the
//! font's `glyf`, `CFF`, or `CFF2` table and converts it into a sequence of
//! [`PathCommand`] values scaled to pixel coordinates.

use ttf_parser::{Face, GlyphId, OutlineBuilder};

/// A single path command in a glyph outline.
#[derive(Debug, Clone)]
pub enum PathCommand {
    /// Move the current point without drawing.
    MoveTo {
        /// Target X in pixels.
        x: f32,
        /// Target Y in pixels.
        y: f32,
    },
    /// Draw a straight line to the given point.
    LineTo {
        /// Target X in pixels.
        x: f32,
        /// Target Y in pixels.
        y: f32,
    },
    /// Draw a quadratic Bézier curve.
    QuadTo {
        /// Control-point X in pixels.
        x1: f32,
        /// Control-point Y in pixels.
        y1: f32,
        /// End-point X in pixels.
        x: f32,
        /// End-point Y in pixels.
        y: f32,
    },
    /// Draw a cubic Bézier curve.
    CubicTo {
        /// First control-point X in pixels.
        x1: f32,
        /// First control-point Y in pixels.
        y1: f32,
        /// Second control-point X in pixels.
        x2: f32,
        /// Second control-point Y in pixels.
        y2: f32,
        /// End-point X in pixels.
        x: f32,
        /// End-point Y in pixels.
        y: f32,
    },
    /// Close the current sub-path.
    Close,
}

/// A glyph outline as a sequence of path commands, scaled to pixels.
#[derive(Debug, Clone, Default)]
pub struct GlyphOutline {
    /// Path commands making up the outline contours.
    pub commands: Vec<PathCommand>,
    /// Bounding box in pixels: `(x_min, y_min, x_max, y_max)`.
    ///
    /// `None` when there are no contours (e.g. whitespace glyphs).
    pub bounding_box: Option<(f32, f32, f32, f32)>,
}

/// Extract the outline of a glyph scaled to pixel coordinates.
///
/// - `face_data`: raw TTF/OTF/TTC bytes.
/// - `glyph_id`: OpenType glyph index.
/// - `scale`: pixels per design unit, typically `font_size_px / units_per_em`.
///
/// The Y axis is flipped from font design space (up) to screen space (down) by
/// multiplying all Y coordinates by `-scale`.
///
/// Returns `None` if the font cannot be parsed, the glyph has no outline (e.g.
/// whitespace, missing glyph), or no commands were produced.
pub fn extract_glyph_outline(face_data: &[u8], glyph_id: u16, scale: f32) -> Option<GlyphOutline> {
    let face = Face::parse(face_data, 0).ok()?;
    let gid = GlyphId(glyph_id);

    let mut builder = ScaledOutlineBuilder::new(scale);
    // outline_glyph returns Some(Rect) when the glyph has outline data; None
    // means "no outline" (whitespace, etc.).
    let _rect = face.outline_glyph(gid, &mut builder)?;

    if builder.commands.is_empty() {
        return None;
    }

    let bbox = compute_bbox(&builder.commands);
    Some(GlyphOutline {
        commands: builder.commands,
        bounding_box: bbox,
    })
}

// ---------------------------------------------------------------------------
// Internal builder
// ---------------------------------------------------------------------------

struct ScaledOutlineBuilder {
    scale: f32,
    commands: Vec<PathCommand>,
}

impl ScaledOutlineBuilder {
    fn new(scale: f32) -> Self {
        Self {
            scale,
            commands: Vec::new(),
        }
    }

    /// Scale X: multiply by `+scale`.
    #[inline]
    fn sx(&self, v: f32) -> f32 {
        v * self.scale
    }

    /// Scale Y: multiply by `-scale` to flip from font-up to screen-down.
    #[inline]
    fn sy(&self, v: f32) -> f32 {
        v * -self.scale
    }
}

impl OutlineBuilder for ScaledOutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(PathCommand::MoveTo {
            x: self.sx(x),
            y: self.sy(y),
        });
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(PathCommand::LineTo {
            x: self.sx(x),
            y: self.sy(y),
        });
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.commands.push(PathCommand::QuadTo {
            x1: self.sx(x1),
            y1: self.sy(y1),
            x: self.sx(x),
            y: self.sy(y),
        });
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.commands.push(PathCommand::CubicTo {
            x1: self.sx(x1),
            y1: self.sy(y1),
            x2: self.sx(x2),
            y2: self.sy(y2),
            x: self.sx(x),
            y: self.sy(y),
        });
    }

    fn close(&mut self) {
        self.commands.push(PathCommand::Close);
    }
}

// ---------------------------------------------------------------------------
// Bounding-box helper
// ---------------------------------------------------------------------------

/// Compute the axis-aligned bounding box by scanning all coordinate values
/// in the path, including control points.
///
/// Returns `None` if `commands` contains no coordinate data.
fn compute_bbox(commands: &[PathCommand]) -> Option<(f32, f32, f32, f32)> {
    let mut x_min = f32::INFINITY;
    let mut y_min = f32::INFINITY;
    let mut x_max = f32::NEG_INFINITY;
    let mut y_max = f32::NEG_INFINITY;

    for cmd in commands {
        let coords: &[f32] = match cmd {
            PathCommand::MoveTo { x, y } => &[*x, *y],
            PathCommand::LineTo { x, y } => &[*x, *y],
            PathCommand::QuadTo { x1, y1, x, y } => &[*x1, *y1, *x, *y],
            PathCommand::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => &[*x1, *y1, *x2, *y2, *x, *y],
            PathCommand::Close => &[],
        };

        // Pairs: (x, y)
        let mut i = 0;
        while i + 1 < coords.len() {
            let cx = coords[i];
            let cy = coords[i + 1];
            if cx < x_min {
                x_min = cx;
            }
            if cx > x_max {
                x_max = cx;
            }
            if cy < y_min {
                y_min = cy;
            }
            if cy > y_max {
                y_max = cy;
            }
            i += 2;
        }
    }

    if x_min.is_infinite() {
        None
    } else {
        Some((x_min, y_min, x_max, y_max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_none_for_no_coords() {
        let commands = vec![PathCommand::Close];
        assert!(compute_bbox(&commands).is_none());
    }

    #[test]
    fn bbox_single_point() {
        let commands = vec![PathCommand::MoveTo { x: 1.0, y: 2.0 }];
        let bbox = compute_bbox(&commands);
        assert!(bbox.is_some());
        let (x0, y0, x1, y1) = bbox.unwrap();
        assert!((x0 - 1.0).abs() < 1e-6);
        assert!((y0 - 2.0).abs() < 1e-6);
        assert!((x1 - 1.0).abs() < 1e-6);
        assert!((y1 - 2.0).abs() < 1e-6);
    }

    #[test]
    fn outline_none_for_empty_data() {
        assert!(extract_glyph_outline(&[], 0, 1.0).is_none());
    }
}
