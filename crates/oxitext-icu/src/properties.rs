//! Unicode character property queries via `icu_properties`.
//!
//! Provides script detection (for shaping itemization) and common per-character
//! property predicates (alphabetic, numeric, whitespace, general category)
//! backed by ICU4X compiled UCD data.
//!
//! # Examples
//!
//! ```rust
//! use oxitext_icu::{CharProperties, TextScript};
//!
//! let props = CharProperties::new();
//! assert_eq!(props.script('木'), TextScript::Han);
//! assert_eq!(props.script('A'), TextScript::Latin);
//! assert!(props.is_alphabetic('Ä'));
//! assert!(props.is_numeric('7'));
//! assert!(props.is_whitespace(' '));
//! ```

use icu_properties::props::{Alphabetic, GeneralCategory, Script, WhiteSpace};
use icu_properties::{
    CodePointMapData, CodePointMapDataBorrowed, CodePointSetData, CodePointSetDataBorrowed,
};

/// A simplified Unicode script classification covering the scripts most
/// relevant to text shaping itemization.
///
/// `Common` covers script-neutral characters (digits, punctuation, spaces);
/// `Inherited` covers combining marks that take their script from the
/// preceding base character; `Other` is any script not enumerated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextScript {
    /// Latin (incl. ASCII letters and Latin-1/Extended).
    Latin,
    /// Greek.
    Greek,
    /// Cyrillic.
    Cyrillic,
    /// Arabic (RTL, requires joining behaviour).
    Arabic,
    /// Hebrew (RTL).
    Hebrew,
    /// Han (CJK ideographs).
    Han,
    /// Hiragana (Japanese).
    Hiragana,
    /// Katakana (Japanese).
    Katakana,
    /// Hangul (Korean).
    Hangul,
    /// Thai.
    Thai,
    /// Devanagari.
    Devanagari,
    /// Script-neutral characters (digits, punctuation, spaces, symbols).
    Common,
    /// Combining marks that inherit their script from the base character.
    Inherited,
    /// Any other script not enumerated above.
    Other,
}

impl TextScript {
    /// Returns `true` if this script is written right-to-left.
    pub fn is_rtl(self) -> bool {
        matches!(self, TextScript::Arabic | TextScript::Hebrew)
    }

    /// Map an ICU4X [`Script`] value to a [`TextScript`].
    fn from_icu(script: Script) -> Self {
        match script {
            Script::Latin => TextScript::Latin,
            Script::Greek => TextScript::Greek,
            Script::Cyrillic => TextScript::Cyrillic,
            Script::Arabic => TextScript::Arabic,
            Script::Hebrew => TextScript::Hebrew,
            Script::Han => TextScript::Han,
            Script::Hiragana => TextScript::Hiragana,
            Script::Katakana => TextScript::Katakana,
            Script::Hangul => TextScript::Hangul,
            Script::Thai => TextScript::Thai,
            Script::Devanagari => TextScript::Devanagari,
            Script::Common => TextScript::Common,
            Script::Inherited => TextScript::Inherited,
            _ => TextScript::Other,
        }
    }
}

/// A contiguous run of text sharing a single [`TextScript`].
///
/// Produced by [`CharProperties::itemize`]; the `start`/`end` fields are UTF-8
/// byte offsets into the analysed string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptRun {
    /// Byte offset of the run start (inclusive).
    pub start: usize,
    /// Byte offset of the run end (exclusive).
    pub end: usize,
    /// The resolved script for the run.
    pub script: TextScript,
}

/// Character property query engine backed by ICU4X compiled UCD data.
///
/// Construction is cheap (borrows static data). Reuse a single instance across
/// many queries.
pub struct CharProperties {
    script: CodePointMapDataBorrowed<'static, Script>,
    general_category: CodePointMapDataBorrowed<'static, GeneralCategory>,
    alphabetic: CodePointSetDataBorrowed<'static>,
    whitespace: CodePointSetDataBorrowed<'static>,
}

impl CharProperties {
    /// Creates a new property query engine.
    pub fn new() -> Self {
        Self {
            script: CodePointMapData::<Script>::new(),
            general_category: CodePointMapData::<GeneralCategory>::new(),
            alphabetic: CodePointSetData::new::<Alphabetic>(),
            whitespace: CodePointSetData::new::<WhiteSpace>(),
        }
    }

    /// Returns the [`TextScript`] of `c`.
    pub fn script(&self, c: char) -> TextScript {
        TextScript::from_icu(self.script.get(c))
    }

    /// Returns `true` if `c` has the Unicode `Alphabetic` property.
    pub fn is_alphabetic(&self, c: char) -> bool {
        self.alphabetic.contains(c)
    }

    /// Returns `true` if `c` has the Unicode `White_Space` property.
    pub fn is_whitespace(&self, c: char) -> bool {
        self.whitespace.contains(c)
    }

    /// Returns `true` if `c` is a decimal-number or other numeric character.
    pub fn is_numeric(&self, c: char) -> bool {
        matches!(
            self.general_category.get(c),
            GeneralCategory::DecimalNumber
                | GeneralCategory::LetterNumber
                | GeneralCategory::OtherNumber
        )
    }

    /// Returns the [`GeneralCategory`] of `c`.
    pub fn general_category(&self, c: char) -> GeneralCategory {
        self.general_category.get(c)
    }

    /// Splits `text` into maximal runs of a single script (script itemization).
    ///
    /// `Common` and `Inherited` characters are merged into the surrounding
    /// run so that, e.g., a space or combining mark between two Latin words
    /// does not start a new run. A leading `Common`/`Inherited` prefix takes
    /// the script of the first strong character that follows.
    ///
    /// Returns an empty `Vec` for empty input.
    pub fn itemize(&self, text: &str) -> Vec<ScriptRun> {
        let mut runs: Vec<ScriptRun> = Vec::new();
        if text.is_empty() {
            return runs;
        }

        let mut current_script: Option<TextScript> = None;
        let mut run_start = 0usize;

        for (idx, c) in text.char_indices() {
            let s = self.script(c);
            // Common/Inherited do not change the active script.
            let resolved = match s {
                TextScript::Common | TextScript::Inherited => current_script,
                strong => Some(strong),
            };

            match (current_script, resolved) {
                (None, Some(strong)) => {
                    // First strong char: backfill any leading neutral prefix.
                    current_script = Some(strong);
                    // run_start stays at 0 (or wherever the neutrals began).
                }
                (Some(prev), Some(strong)) if prev != strong => {
                    // Script boundary: close the current run.
                    runs.push(ScriptRun {
                        start: run_start,
                        end: idx,
                        script: prev,
                    });
                    run_start = idx;
                    current_script = Some(strong);
                }
                _ => {
                    // Same script or still in a neutral prefix — extend run.
                }
            }
        }

        // Close the final run.
        let final_script = current_script.unwrap_or(TextScript::Common);
        runs.push(ScriptRun {
            start: run_start,
            end: text.len(),
            script: final_script,
        });
        runs
    }

    /// Returns the dominant (most frequent strong) script in `text`, or
    /// `TextScript::Common` if the text has no strong characters.
    pub fn dominant_script(&self, text: &str) -> TextScript {
        use std::collections::HashMap;
        let mut counts: HashMap<TextScript, usize> = HashMap::new();
        for c in text.chars() {
            let s = self.script(c);
            if !matches!(s, TextScript::Common | TextScript::Inherited) {
                *counts.entry(s).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(s, _)| s)
            .unwrap_or(TextScript::Common)
    }

    /// Returns `true` if `text` contains any right-to-left script characters.
    pub fn has_rtl(&self, text: &str) -> bool {
        text.chars().any(|c| self.script(c).is_rtl())
    }
}

impl Default for CharProperties {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_scripts() {
        let p = CharProperties::new();
        assert_eq!(p.script('A'), TextScript::Latin);
        assert_eq!(p.script('Ω'), TextScript::Greek);
        assert_eq!(p.script('Я'), TextScript::Cyrillic);
        assert_eq!(p.script('木'), TextScript::Han);
        assert_eq!(p.script('あ'), TextScript::Hiragana);
        assert_eq!(p.script('ア'), TextScript::Katakana);
        assert_eq!(p.script('한'), TextScript::Hangul);
        assert_eq!(p.script('ก'), TextScript::Thai);
    }

    #[test]
    fn detects_rtl_scripts() {
        let p = CharProperties::new();
        assert_eq!(p.script('ا'), TextScript::Arabic);
        assert!(p.script('ا').is_rtl());
        assert_eq!(p.script('א'), TextScript::Hebrew);
        assert!(p.script('א').is_rtl());
        assert!(!p.script('A').is_rtl());
    }

    #[test]
    fn common_and_neutral_classification() {
        let p = CharProperties::new();
        assert_eq!(p.script('5'), TextScript::Common);
        assert_eq!(p.script(' '), TextScript::Common);
        assert_eq!(p.script('.'), TextScript::Common);
    }

    #[test]
    fn property_predicates() {
        let p = CharProperties::new();
        assert!(p.is_alphabetic('A'));
        assert!(p.is_alphabetic('Ä'));
        assert!(!p.is_alphabetic('3'));
        assert!(p.is_numeric('7'));
        assert!(!p.is_numeric('A'));
        assert!(p.is_whitespace(' '));
        assert!(p.is_whitespace('\t'));
        assert!(!p.is_whitespace('x'));
    }

    #[test]
    fn itemize_pure_latin_one_run() {
        let p = CharProperties::new();
        let runs = p.itemize("hello");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].script, TextScript::Latin);
        assert_eq!(runs[0].start, 0);
        assert_eq!(runs[0].end, 5);
    }

    #[test]
    fn itemize_merges_spaces_into_latin() {
        let p = CharProperties::new();
        // The space between two Latin words must NOT split the run.
        let runs = p.itemize("hi yo");
        assert_eq!(runs.len(), 1, "space should not break a same-script run");
        assert_eq!(runs[0].script, TextScript::Latin);
    }

    #[test]
    fn itemize_splits_latin_and_han() {
        let p = CharProperties::new();
        let text = "abc木字";
        let runs = p.itemize(text);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].script, TextScript::Latin);
        assert_eq!(runs[0].start, 0);
        assert_eq!(runs[0].end, 3); // "abc" = 3 bytes
        assert_eq!(runs[1].script, TextScript::Han);
        assert_eq!(runs[1].start, 3);
        assert_eq!(runs[1].end, text.len());
    }

    #[test]
    fn itemize_empty_is_empty() {
        let p = CharProperties::new();
        assert!(p.itemize("").is_empty());
    }

    #[test]
    fn itemize_leading_neutral_takes_following_script() {
        let p = CharProperties::new();
        // Leading digits then Latin → single Latin run covering everything.
        let runs = p.itemize("12ab");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].script, TextScript::Latin);
        assert_eq!(runs[0].start, 0);
    }

    #[test]
    fn dominant_script_picks_majority() {
        let p = CharProperties::new();
        assert_eq!(p.dominant_script("abc木"), TextScript::Latin);
        assert_eq!(p.dominant_script("木字宙abc語"), TextScript::Han);
        assert_eq!(p.dominant_script("123 456"), TextScript::Common);
    }

    #[test]
    fn has_rtl_detection() {
        let p = CharProperties::new();
        assert!(p.has_rtl("hello مرحبا"));
        assert!(!p.has_rtl("hello world"));
    }
}
