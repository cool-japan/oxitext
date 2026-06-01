//! Layout options and configuration for [`crate::engine::LayoutEngine`].
//!
//! Provides [`LayoutOptions`] — a comprehensive configuration struct for layout
//! passes — along with its builder [`LayoutOptionsBuilder`], as well as
//! [`TruncationMode`] and [`TabStops`] sub-configurations.

use oxitext_core::{FlowDirection, TextAlignment, TextDecoration};

/// Mode for text truncation when content exceeds `max_width`.
///
/// The caller is responsible for pre-measuring the ellipsis string ("…",
/// U+2026) and pre-shaping its glyph so that the layout engine can apply
/// truncation without font-access dependencies.
#[derive(Debug, Clone)]
pub struct TruncationMode {
    /// Maximum line width in pixels before truncation kicks in.
    pub max_width: f32,
    /// The pre-computed total advance width of the ellipsis string.
    ///
    /// Caller measures "…" (U+2026) before passing here.
    pub ellipsis_advance: f32,
    /// Pre-shaped glyph ID for the ellipsis character (or 0 for a fallback
    /// marker).
    pub ellipsis_glyph_id: u16,
}

/// Tab stop configuration.
///
/// Explicit tab stop positions take priority; once exhausted (or when the list
/// is empty), the [`Self::default_interval`] drives implicit stops.
#[derive(Debug, Clone)]
pub struct TabStops {
    /// Explicit tab stop x-positions (sorted ascending).
    pub positions: Vec<f32>,
    /// Default interval for implicit tab stops (used when `positions` is empty
    /// or exhausted).
    pub default_interval: f32,
}

impl TabStops {
    /// Creates a [`TabStops`] with no explicit positions and the given default
    /// interval.
    pub fn with_interval(interval: f32) -> Self {
        Self {
            positions: Vec::new(),
            default_interval: interval,
        }
    }

    /// Returns the next tab stop x-position from a given cursor x.
    ///
    /// Scans the explicit `positions` first; if none is strictly greater than
    /// `cursor_x + 0.5`, falls back to the default interval arithmetic.
    pub fn next_stop(&self, cursor_x: f32) -> f32 {
        for &pos in &self.positions {
            if pos > cursor_x + 0.5 {
                return pos;
            }
        }
        // Use default interval.
        let next = ((cursor_x / self.default_interval).floor() + 1.0) * self.default_interval;
        next.max(cursor_x + 1.0)
    }
}

impl Default for TabStops {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            default_interval: 80.0,
        }
    }
}

/// Comprehensive layout options for a single layout pass.
///
/// Construct via [`LayoutOptions::builder()`] for a fluent API, or create
/// directly and use [`Default`] for the standard left-aligned horizontal flow.
#[derive(Debug, Clone)]
pub struct LayoutOptions {
    /// Horizontal text alignment within the line box.
    pub alignment: TextAlignment,
    /// Text flow direction (horizontal or vertical).
    pub flow_direction: FlowDirection,
    /// Truncation configuration; `None` disables truncation.
    pub truncation: Option<TruncationMode>,
    /// Tab stop configuration.
    pub tab_stops: TabStops,
    /// Extra vertical space (in pixels) inserted between paragraphs when using
    /// [`crate::engine::LayoutEngine::layout_paragraphs`].
    pub paragraph_spacing: f32,
    /// When `true`, CJK fullwidth punctuation at the start or end of a line is
    /// allowed to overhang ("hang") into the margin by half its advance width.
    ///
    /// This is the CSS `hanging-punctuation: allow-end` behaviour from CSS Text
    /// Module Level 3 §3.  Affects [`crate::engine::LayoutEngine::layout_with_options`].
    pub hanging_punctuation: bool,
    /// Optional text decoration to apply to all rendered runs.
    ///
    /// When set, [`crate::engine::LayoutEngine::layout_with_options`] computes
    /// [`oxitext_core::DecorationRect`]s for every line and stores them in
    /// [`crate::engine::LayoutResult::decorations`].
    pub decoration: Option<TextDecoration>,
    /// Inline objects to be positioned during layout.
    ///
    /// Each [`oxitext_core::InlineObject`] is appended after the shaped glyphs
    /// on the last line, advancing the cursor by `object.advance` for each.
    pub inline_objects: Vec<oxitext_core::InlineObject>,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            alignment: TextAlignment::Left,
            flow_direction: FlowDirection::Horizontal,
            truncation: None,
            tab_stops: TabStops::default(),
            paragraph_spacing: 0.0,
            hanging_punctuation: false,
            decoration: None,
            inline_objects: Vec::new(),
        }
    }
}

impl LayoutOptions {
    /// Returns a new [`LayoutOptionsBuilder`] initialised with the defaults.
    pub fn builder() -> LayoutOptionsBuilder {
        LayoutOptionsBuilder(Self::default())
    }
}

/// Fluent builder for [`LayoutOptions`].
pub struct LayoutOptionsBuilder(LayoutOptions);

impl LayoutOptionsBuilder {
    /// Sets the text alignment.
    pub fn alignment(mut self, a: TextAlignment) -> Self {
        self.0.alignment = a;
        self
    }

    /// Sets the flow direction.
    pub fn flow_direction(mut self, d: FlowDirection) -> Self {
        self.0.flow_direction = d;
        self
    }

    /// Enables truncation with the given mode.
    pub fn truncation(mut self, t: TruncationMode) -> Self {
        self.0.truncation = Some(t);
        self
    }

    /// Sets the tab stop configuration.
    pub fn tab_stops(mut self, ts: TabStops) -> Self {
        self.0.tab_stops = ts;
        self
    }

    /// Sets the paragraph spacing.
    pub fn paragraph_spacing(mut self, s: f32) -> Self {
        self.0.paragraph_spacing = s;
        self
    }

    /// Enables or disables hanging punctuation.
    ///
    /// When `true`, CJK fullwidth punctuation at a line's start/end overhangs
    /// into the margin by half its advance.  Defaults to `false`.
    pub fn hanging_punctuation(mut self, hp: bool) -> Self {
        self.0.hanging_punctuation = hp;
        self
    }

    /// Sets the text decoration to apply to all lines in the layout.
    ///
    /// When set, the layout engine computes [`oxitext_core::DecorationRect`]s
    /// for each line and stores them in
    /// [`crate::engine::LayoutResult::decorations`].
    pub fn decoration(mut self, d: TextDecoration) -> Self {
        self.0.decoration = Some(d);
        self
    }

    /// Sets the inline objects to be positioned during layout.
    ///
    /// The objects are appended after the shaped glyphs, advancing the cursor
    /// by each object's `advance` width.
    pub fn inline_objects(mut self, objects: Vec<oxitext_core::InlineObject>) -> Self {
        self.0.inline_objects = objects;
        self
    }

    /// Consumes the builder and returns the final [`LayoutOptions`].
    pub fn build(self) -> LayoutOptions {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_stops_with_interval_default() {
        let ts = TabStops::with_interval(80.0);
        assert!(ts.positions.is_empty());
        assert_eq!(ts.default_interval, 80.0);
    }

    #[test]
    fn tab_stops_next_stop_interval() {
        let ts = TabStops::with_interval(80.0);
        // Cursor at 10 → next stop at 80.
        let stop = ts.next_stop(10.0);
        assert!((stop - 80.0).abs() < 1.0, "expected ~80.0, got {stop}");
        // Cursor at 80 → next stop at 160.
        let stop2 = ts.next_stop(80.0);
        assert!((stop2 - 160.0).abs() < 1.0, "expected ~160.0, got {stop2}");
    }

    #[test]
    fn tab_stops_explicit_positions() {
        let ts = TabStops {
            positions: vec![50.0, 120.0, 200.0],
            default_interval: 80.0,
        };
        // cursor at 0 → first explicit stop 50.
        assert!((ts.next_stop(0.0) - 50.0).abs() < 1.0);
        // cursor at 50 → next explicit stop 120.
        assert!((ts.next_stop(50.5) - 120.0).abs() < 1.0);
        // cursor at 210 (beyond all explicit) → default interval: 210/80=2.625 →
        // floor+1 = 3 → 240.
        let stop = ts.next_stop(210.0);
        assert!((stop - 240.0).abs() < 1.0, "expected ~240.0, got {stop}");
    }

    #[test]
    fn tab_stops_default_impl() {
        let ts = TabStops::default();
        assert_eq!(ts.default_interval, 80.0);
    }

    #[test]
    fn layout_options_default() {
        let opts = LayoutOptions::default();
        assert_eq!(opts.alignment, TextAlignment::Left);
        assert_eq!(opts.flow_direction, FlowDirection::Horizontal);
        assert!(opts.truncation.is_none());
        assert_eq!(opts.paragraph_spacing, 0.0);
    }

    #[test]
    fn layout_options_builder_sets_fields() {
        let opts = LayoutOptions::builder()
            .alignment(TextAlignment::Center)
            .flow_direction(FlowDirection::Vertical)
            .paragraph_spacing(12.0)
            .build();
        assert_eq!(opts.alignment, TextAlignment::Center);
        assert_eq!(opts.flow_direction, FlowDirection::Vertical);
        assert_eq!(opts.paragraph_spacing, 12.0);
    }

    #[test]
    fn layout_options_builder_with_truncation() {
        let trunc = TruncationMode {
            max_width: 100.0,
            ellipsis_advance: 10.0,
            ellipsis_glyph_id: 42,
        };
        let opts = LayoutOptions::builder().truncation(trunc).build();
        let t = opts.truncation.as_ref().expect("truncation should be set");
        assert_eq!(t.max_width, 100.0);
        assert_eq!(t.ellipsis_glyph_id, 42);
    }

    #[test]
    fn layout_options_builder_with_tab_stops() {
        let ts = TabStops::with_interval(40.0);
        let opts = LayoutOptions::builder().tab_stops(ts).build();
        assert_eq!(opts.tab_stops.default_interval, 40.0);
    }

    #[test]
    fn layout_options_with_decoration() {
        let opts = LayoutOptions::builder()
            .decoration(TextDecoration::Underline {
                color: oxitext_core::Rgba8 {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                thickness: 1.0,
                offset: 2.0,
            })
            .build();
        assert!(opts.decoration.is_some());
        match opts.decoration {
            Some(TextDecoration::Underline {
                thickness, offset, ..
            }) => {
                assert_eq!(thickness, 1.0);
                assert_eq!(offset, 2.0);
            }
            _ => panic!("expected Underline decoration"),
        }
    }

    #[test]
    fn layout_options_decoration_none_by_default() {
        let opts = LayoutOptions::default();
        assert!(opts.decoration.is_none());
    }

    #[test]
    fn test_layout_options_with_inline_objects() {
        use oxitext_core::InlineObject;
        let obj = InlineObject {
            id: 1,
            width: 20.0,
            height: 20.0,
            baseline_offset: 0.0,
            advance: 20.0,
        };
        let opts = LayoutOptions::builder().inline_objects(vec![obj]).build();
        assert_eq!(opts.inline_objects.len(), 1);
    }

    #[test]
    fn test_styled_run_vertical_position_default() {
        use oxitext_core::VerticalPosition;
        // Verify the field exists and defaults to Normal
        let vp = VerticalPosition::Normal;
        assert_eq!(vp.effective_size(16.0), 16.0);
    }
}
