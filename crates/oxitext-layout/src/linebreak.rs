//! UAX #14 Unicode Line Breaking Algorithm.
//!
//! Wraps the `unicode-linebreak` crate to expose a typed [`LineBreaker`]
//! iterator over [`LineBreak`] opportunities in a string.

use unicode_linebreak::{linebreaks, BreakOpportunity};

/// A line-break opportunity classified by urgency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineBreak {
    /// A mandatory line break (e.g. `U+000A` NEWLINE, `U+0085` NEL).
    Mandatory,
    /// An optional line break that a layout engine may take when wrapping.
    Allowed,
}

/// Pre-collected line-break opportunities for a string.
///
/// Because `unicode_linebreak::linebreaks()` returns an anonymous iterator
/// type, all break opportunities are collected eagerly into a `Vec` and
/// exposed through [`IntoIterator`] and an index-based slice accessor.
///
/// Each item is `(byte_offset, LineBreak)` where `byte_offset` is the
/// position *after* the last character of the breakable unit (exclusive end
/// of the pre-break segment).
pub struct LineBreaker {
    breaks: Vec<(usize, LineBreak)>,
}

impl LineBreaker {
    /// Analyse `text` and collect all line-break opportunities.
    pub fn new(text: &str) -> Self {
        let breaks = linebreaks(text)
            .map(|(pos, opp)| {
                let lb = match opp {
                    BreakOpportunity::Mandatory => LineBreak::Mandatory,
                    BreakOpportunity::Allowed => LineBreak::Allowed,
                };
                (pos, lb)
            })
            .collect();
        Self { breaks }
    }

    /// Returns all collected break opportunities as a slice.
    pub fn breaks(&self) -> &[(usize, LineBreak)] {
        &self.breaks
    }

    /// Returns an iterator over `(byte_offset, LineBreak)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = &(usize, LineBreak)> {
        self.breaks.iter()
    }
}

impl IntoIterator for LineBreaker {
    type Item = (usize, LineBreak);
    type IntoIter = std::vec::IntoIter<(usize, LineBreak)>;

    fn into_iter(self) -> Self::IntoIter {
        self.breaks.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_allows_break() {
        let lb = LineBreaker::new("hello world");
        let breaks: Vec<_> = lb.iter().cloned().collect();
        assert!(
            !breaks.is_empty(),
            "should have at least one break opportunity"
        );
    }

    #[test]
    fn newline_is_mandatory() {
        let lb = LineBreaker::new("hello\nworld");
        let mandatory = lb.iter().any(|(_, kind)| *kind == LineBreak::Mandatory);
        assert!(mandatory, "newline should produce a mandatory break");
    }
}
