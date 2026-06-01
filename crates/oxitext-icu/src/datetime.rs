//! Locale-aware date/time formatting via ICU4X.
//!
//! Wraps the ICU4X [`icu_datetime::DateTimeFormatter`] and
//! [`icu_datetime::NoCalendarFormatter`] types with a simple, ergonomic API that
//! accepts plain integers for year/month/day/hour/minute/second and returns owned
//! `String` values.
//!
//! # Examples
//!
//! ```rust
//! use oxitext_icu::IcuDateTimeFormatter;
//!
//! let fmt = IcuDateTimeFormatter::new("en").expect("English formatter");
//! let s = fmt.format_date(2025, 5, 25);
//! assert!(!s.is_empty());
//! ```

use icu_datetime::{
    fieldsets::{T, YMD, YMDT},
    input::{Date, DateTime, Time},
    options::Length,
    DateTimeFormatter, DateTimeFormatterPreferences, NoCalendarFormatter,
};
use icu_locale_core::Locale;

use crate::CollateError;

/// Controls how much detail is shown for date formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateLength {
    /// Widest date format (mapped to `Long` in ICU4X which has no separate "Full" length).
    Full,
    /// Long date format (e.g. "May 25, 2025").
    Long,
    /// Medium date format (e.g. "May 25, 2025").
    #[default]
    Medium,
    /// Short date format (e.g. "5/25/25").
    Short,
}

/// Controls how much detail is shown for time formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeLength {
    /// Widest time format (mapped to `Long` in ICU4X which has no separate "Full" length).
    Full,
    /// Long time format (e.g. "3:47:50 PM PDT").
    Long,
    /// Medium time format (e.g. "3:47:50 PM").
    #[default]
    Medium,
    /// Short time format (e.g. "3:47 PM").
    Short,
    /// No time component — date-only formatting.
    None,
}

impl DateLength {
    /// Converts this variant to the ICU4X [`Length`] value.
    ///
    /// `DateLength::Full` is mapped to `Length::Long` because ICU4X's `options::Length`
    /// does not have a distinct "Full" value.
    fn to_icu_length(self) -> Length {
        match self {
            Self::Full | Self::Long => Length::Long,
            Self::Medium => Length::Medium,
            Self::Short => Length::Short,
        }
    }
}

impl TimeLength {
    /// Converts this variant to the ICU4X [`Length`] value.
    ///
    /// `TimeLength::Full` is mapped to `Length::Long`.
    /// `TimeLength::None` should be handled by the caller before calling this.
    fn to_icu_length(self) -> Length {
        match self {
            Self::Full | Self::Long => Length::Long,
            Self::Medium => Length::Medium,
            Self::Short => Length::Short,
            Self::None => Length::Short,
        }
    }
}

/// Locale-aware date/time formatter backed by ICU4X compiled CLDR data.
///
/// Holds three compiled formatters:
/// * a date-only formatter (year/month/day)
/// * a time-only formatter (hour/minute/second)
/// * a combined date+time formatter
///
/// Construction is cheap: CLDR data lives in static tables baked into the binary.
///
/// # Examples
///
/// ```rust
/// use oxitext_icu::{IcuDateTimeFormatter, DateLength, TimeLength};
///
/// let fmt = IcuDateTimeFormatter::new_with_lengths("de", DateLength::Long, TimeLength::Short)
///     .expect("German formatter");
/// let s = fmt.format_date(2025, 12, 24);
/// assert!(!s.is_empty());
/// ```
pub struct IcuDateTimeFormatter {
    locale_string: String,
    date_formatter: DateTimeFormatter<YMD>,
    time_formatter: NoCalendarFormatter<T>,
    datetime_formatter: DateTimeFormatter<YMDT>,
}

impl IcuDateTimeFormatter {
    /// Creates a new locale-aware formatter with default [`DateLength::Medium`] and
    /// [`TimeLength::Medium`].
    ///
    /// `locale_id` is a BCP-47 locale string, e.g. `"en"`, `"ja"`, `"de"`.
    ///
    /// # Errors
    ///
    /// Returns [`CollateError`] if the locale string cannot be parsed or ICU data
    /// loading fails.
    pub fn new(locale_id: &str) -> Result<Self, CollateError> {
        Self::new_with_lengths(locale_id, DateLength::default(), TimeLength::default())
    }

    /// Creates a new locale-aware formatter with explicit date and time lengths.
    ///
    /// # Errors
    ///
    /// Returns [`CollateError`] if the locale string cannot be parsed or if any of
    /// the three ICU formatters fail to initialise.
    pub fn new_with_lengths(
        locale_id: &str,
        date_length: DateLength,
        time_length: TimeLength,
    ) -> Result<Self, CollateError> {
        let locale: Locale = locale_id
            .parse()
            .map_err(|e| CollateError::InvalidLocale(format!("{e}")))?;

        let dl = date_length.to_icu_length();
        let tl = time_length.to_icu_length();

        let prefs = DateTimeFormatterPreferences::from(locale);

        let date_formatter = DateTimeFormatter::try_new(prefs, YMD::for_length(dl))
            .map_err(|e| CollateError::Icu(format!("date formatter: {e}")))?;

        let time_formatter = NoCalendarFormatter::try_new(prefs, T::for_length(tl))
            .map_err(|e| CollateError::Icu(format!("time formatter: {e}")))?;

        // For the combined formatter, use date length; the time portion inherits
        // from the same field-set options via YMDT.
        let datetime_formatter = DateTimeFormatter::try_new(prefs, YMDT::for_length(dl))
            .map_err(|e| CollateError::Icu(format!("datetime formatter: {e}")))?;

        Ok(Self {
            locale_string: locale_id.to_owned(),
            date_formatter,
            time_formatter,
            datetime_formatter,
        })
    }

    /// Formats a Gregorian date (year, month 1-12, day 1-31) using the locale.
    ///
    /// Falls back to ISO 8601 (`YYYY-MM-DD`) if the date values are out of range.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuDateTimeFormatter;
    ///
    /// let fmt = IcuDateTimeFormatter::new("en").expect("formatter");
    /// let s = fmt.format_date(2025, 5, 25);
    /// assert!(s.contains("2025") || s.contains("25"));
    /// ```
    pub fn format_date(&self, year: i32, month: u8, day: u8) -> String {
        match Date::try_new_iso(year, month, day) {
            Ok(date) => self.date_formatter.format(&date).to_string(),
            Err(_) => format!("{year:04}-{month:02}-{day:02}"),
        }
    }

    /// Formats a time (hour 0-23, minute 0-59, second 0-59) using the locale.
    ///
    /// Falls back to `HH:MM:SS` if the time values are out of range.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuDateTimeFormatter;
    ///
    /// let fmt = IcuDateTimeFormatter::new("en").expect("formatter");
    /// let s = fmt.format_time(14, 30, 0);
    /// assert!(!s.is_empty());
    /// ```
    pub fn format_time(&self, hour: u8, minute: u8, second: u8) -> String {
        match Time::try_new(hour, minute, second, 0) {
            Ok(time) => self.time_formatter.format(&time).to_string(),
            Err(_) => format!("{hour:02}:{minute:02}:{second:02}"),
        }
    }

    /// Formats a full date+time (year, month, day, hour, minute, second).
    ///
    /// Falls back to ISO 8601 combined format if values are out of range.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxitext_icu::IcuDateTimeFormatter;
    ///
    /// let fmt = IcuDateTimeFormatter::new("en").expect("formatter");
    /// let s = fmt.format_datetime(2025, 5, 25, 14, 30, 0);
    /// assert!(!s.is_empty());
    /// ```
    pub fn format_datetime(
        &self,
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> String {
        let date = Date::try_new_iso(year, month, day);
        let time = Time::try_new(hour, minute, second, 0);
        match (date, time) {
            (Ok(d), Ok(t)) => {
                let dt = DateTime { date: d, time: t };
                self.datetime_formatter.format(&dt).to_string()
            }
            _ => format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"),
        }
    }

    /// Returns the locale identifier string that was used to construct this formatter.
    pub fn locale_id(&self) -> &str {
        &self.locale_string
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datetime_formatter_basic() {
        let fmt = IcuDateTimeFormatter::new("en").expect("en formatter");
        let s = fmt.format_date(2025, 5, 25);
        // Should contain 2025 or 25 in some form.
        assert!(
            s.contains("2025") || s.contains("25"),
            "unexpected date string: {s}"
        );
    }

    #[test]
    fn test_datetime_formatter_japanese() {
        let fmt = IcuDateTimeFormatter::new("ja").expect("ja formatter");
        let s = fmt.format_date(2025, 1, 7);
        // Japanese date should not be empty.
        assert!(!s.is_empty(), "Japanese date format should not be empty");
    }

    #[test]
    fn locale_id_round_trips() {
        let fmt = IcuDateTimeFormatter::new("de").expect("German formatter");
        assert_eq!(fmt.locale_id(), "de");
    }

    #[test]
    fn format_time_basic() {
        let fmt = IcuDateTimeFormatter::new("en").expect("en formatter");
        let s = fmt.format_time(14, 30, 0);
        assert!(!s.is_empty(), "time format should not be empty: {s}");
    }

    #[test]
    fn format_datetime_basic() {
        let fmt = IcuDateTimeFormatter::new("en").expect("en formatter");
        let s = fmt.format_datetime(2025, 5, 25, 14, 30, 0);
        assert!(!s.is_empty(), "datetime format should not be empty: {s}");
    }

    #[test]
    fn out_of_range_date_falls_back() {
        let fmt = IcuDateTimeFormatter::new("en").expect("en formatter");
        // month 0 is invalid.
        let s = fmt.format_date(2025, 0, 1);
        assert_eq!(s, "2025-00-01", "fallback format unexpected: {s}");
    }

    #[test]
    fn out_of_range_time_falls_back() {
        let fmt = IcuDateTimeFormatter::new("en").expect("en formatter");
        // hour 25 is invalid.
        let s = fmt.format_time(25, 0, 0);
        assert_eq!(s, "25:00:00", "fallback format unexpected: {s}");
    }

    #[test]
    fn new_with_lengths_short() {
        let fmt =
            IcuDateTimeFormatter::new_with_lengths("en", DateLength::Short, TimeLength::Short)
                .expect("short formatter");
        let s = fmt.format_date(2025, 5, 25);
        assert!(!s.is_empty(), "short date should not be empty: {s}");
    }

    #[test]
    fn new_with_lengths_full_maps_to_long() {
        let fmt = IcuDateTimeFormatter::new_with_lengths("en", DateLength::Full, TimeLength::Full)
            .expect("full formatter");
        let s = fmt.format_date(2025, 5, 25);
        assert!(!s.is_empty(), "full->long date should not be empty: {s}");
    }

    #[test]
    fn invalid_locale_returns_error() {
        let result = IcuDateTimeFormatter::new("not-a-valid-locale!!!");
        assert!(result.is_err(), "invalid locale should return error");
    }
}
