//! Rich line-aware layout engine.
//!
//! Split into submodules:
//! - [`types`] — data types: [`BreakingStrategy`], [`LayoutResult`], [`Line`],
//!   [`LineMetrics`], [`ParagraphMetrics`], [`LayoutEngine`], and
//!   `VerticalLineModel`.
//! - [`functions`] — private helper functions used by [`LayoutEngine`] methods.

pub mod functions;
pub mod types;

// Re-export public API from types (all the public structs and enums).
pub use types::{
    BreakingStrategy, LayoutEngine, LayoutResult, Line, LineMetrics, ParagraphMetrics,
};
