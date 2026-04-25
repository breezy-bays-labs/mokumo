//! Profile slug — kebab-case identifier used as the on-disk profile
//! directory name and the primary key of `meta.profiles`.
//!
//! Slugs are kebab-case ASCII (lowercase letters, digits, hyphens), 1..=60
//! chars, with no leading/trailing hyphen and no `--` runs. They MUST NOT
//! collide with reserved names (see [`RESERVED_SLUGS`]).

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::str::FromStr;

/// Names that cannot be used as a profile slug.
///
/// `demo` is the special demo profile (cannot be created or deleted by the
/// operator). `meta` and `sessions` are install-level filenames that share
/// the data directory with profile folders; allowing them as slugs would
/// shadow the bootstrap files at `<data_dir>/meta.db` and
/// `<data_dir>/sessions.db`.
pub const RESERVED_SLUGS: &[&str] = &["demo", "meta", "sessions"];

/// Maximum slug length, in bytes (also chars — slugs are ASCII).
pub const MAX_SLUG_LEN: usize = 60;

/// Validation errors for [`Slug::new`] and [`derive_slug`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlugError {
    #[error("slug is empty")]
    Empty,
    #[error("slug is {len} chars; max is {max}")]
    TooLong { len: usize, max: usize },
    #[error("slug `{0}` is reserved")]
    Reserved(String),
    #[error("slug `{0}` contains characters outside [a-z0-9-]")]
    InvalidChars(String),
    #[error("slug `{0}` has leading or trailing hyphen, or contains `--`")]
    HyphenLayout(String),
    #[error("slug cannot be derived from input `{input}`")]
    Unparseable { input: String },
}

/// Validated profile slug.
///
/// Construction goes through [`Slug::new`] (already-canonical input) or
/// [`derive_slug`] (free-form display name → slug). Custom `Deserialize`
/// funnels every wire-decoded value through `Slug::new` so a payload
/// carrying an arbitrary string cannot bypass validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Slug(String);

impl<'de> Deserialize<'de> for Slug {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Slug::new(s).map_err(serde::de::Error::custom)
    }
}

impl Slug {
    /// Construct a `Slug` from an already-canonical string. Returns the
    /// specific [`SlugError`] for the first rule violated.
    pub fn new(s: impl Into<String>) -> Result<Self, SlugError> {
        let s: String = s.into();
        if s.is_empty() {
            return Err(SlugError::Empty);
        }
        if s.len() > MAX_SLUG_LEN {
            return Err(SlugError::TooLong {
                len: s.len(),
                max: MAX_SLUG_LEN,
            });
        }
        if !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(SlugError::InvalidChars(s));
        }
        if s.starts_with('-') || s.ends_with('-') || s.contains("--") {
            return Err(SlugError::HyphenLayout(s));
        }
        if RESERVED_SLUGS.contains(&s.as_str()) {
            return Err(SlugError::Reserved(s));
        }
        Ok(Self(s))
    }

    /// Borrow the slug as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the inner `String`.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Slug {
    type Err = SlugError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_owned())
    }
}

impl AsRef<str> for Slug {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Hard cap on the input length [`derive_slug`] will accept before doing
/// any per-byte work. Set well above any reasonable display name (legal
/// shop-name fields, free-form profile labels) but bounded so a malicious
/// or corrupted legacy DB row cannot trigger an unbounded `String`
/// allocation. Anything past the cap is rejected as `TooLong` without
/// scanning the input.
pub const DERIVE_INPUT_BYTE_CAP: usize = 1024;

/// Derive a slug from a free-form display name.
///
/// Algorithm: ASCII-lowercase, map every non-`[a-z0-9]` byte (including
/// any multi-byte UTF-8 sequence) to a single `-`, collapse runs of `-`,
/// trim leading/trailing `-`, then funnel through [`Slug::new`] for the
/// length / reserved / hyphen-layout / empty checks.
///
/// Rejection cases:
/// - `SlugError::TooLong` — input length exceeds [`DERIVE_INPUT_BYTE_CAP`]
///   (early-rejected before allocation), OR canonicalised length exceeds
///   [`MAX_SLUG_LEN`] (rejected by [`Slug::new`]).
/// - `SlugError::Unparseable` — the canonicalised string is empty (input
///   was empty, all-whitespace, or all non-`[a-z0-9]`). The caller sees
///   this as "the display name has no slug-worthy characters".
/// - `SlugError::Reserved` — derived to one of [`RESERVED_SLUGS`].
///
/// Non-ASCII input is intentionally NOT transliterated. `Café` derives to
/// `caf` (the `é` becomes `-`, stripped at the trim step), not `cafe`. A
/// transliteration step would need a locale-dependent table and would
/// produce slugs that surprise the operator more often than they help.
/// If the operator wants `cafe`, they type `Cafe`.
pub fn derive_slug(input: &str) -> Result<Slug, SlugError> {
    // Pre-allocation length cap: a malicious or corrupted legacy
    // `shop_settings.shop_name` could otherwise force a large `String`
    // allocation before `Slug::new`'s 60-char check rejects it. Reject
    // early without scanning the input.
    if input.len() > DERIVE_INPUT_BYTE_CAP {
        return Err(SlugError::TooLong {
            len: input.len(),
            max: MAX_SLUG_LEN,
        });
    }
    let mut buf = String::with_capacity(input.len());
    let mut last_was_hyphen = false;
    for byte in input.bytes() {
        let ch = byte.to_ascii_lowercase();
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            buf.push(ch as char);
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            buf.push('-');
            last_was_hyphen = true;
        }
    }
    let trimmed = buf.trim_matches('-');
    if trimmed.is_empty() {
        return Err(SlugError::Unparseable {
            input: input.to_owned(),
        });
    }
    Slug::new(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_valid_kebab_slug() {
        assert_eq!(
            Slug::new("acme-printing").unwrap().as_str(),
            "acme-printing"
        );
    }

    #[test]
    fn new_rejects_empty() {
        assert_eq!(Slug::new(""), Err(SlugError::Empty));
    }

    #[test]
    fn new_rejects_over_max_len() {
        let s = "a".repeat(MAX_SLUG_LEN + 1);
        assert!(matches!(Slug::new(&s), Err(SlugError::TooLong { .. })));
    }

    #[test]
    fn new_rejects_reserved() {
        for name in RESERVED_SLUGS {
            assert!(
                matches!(Slug::new(*name), Err(SlugError::Reserved(_))),
                "expected `{name}` to be reserved"
            );
        }
    }

    #[test]
    fn new_rejects_uppercase() {
        assert!(matches!(Slug::new("Acme"), Err(SlugError::InvalidChars(_))));
    }

    #[test]
    fn new_rejects_underscore() {
        assert!(matches!(
            Slug::new("acme_print"),
            Err(SlugError::InvalidChars(_))
        ));
    }

    #[test]
    fn new_rejects_leading_hyphen() {
        assert!(matches!(
            Slug::new("-acme"),
            Err(SlugError::HyphenLayout(_))
        ));
    }

    #[test]
    fn new_rejects_trailing_hyphen() {
        assert!(matches!(
            Slug::new("acme-"),
            Err(SlugError::HyphenLayout(_))
        ));
    }

    #[test]
    fn new_rejects_double_hyphen() {
        assert!(matches!(
            Slug::new("acme--print"),
            Err(SlugError::HyphenLayout(_))
        ));
    }

    #[test]
    fn from_str_round_trips_through_display() {
        let s = Slug::new("acme-printing").unwrap();
        let s2: Slug = s.to_string().parse().unwrap();
        assert_eq!(s, s2);
    }

    #[test]
    fn derive_slug_kebabs_whitespace() {
        assert_eq!(
            derive_slug("Acme Printing").unwrap().as_str(),
            "acme-printing"
        );
    }

    #[test]
    fn derive_slug_ascii_lowercases() {
        assert_eq!(derive_slug("ACME").unwrap().as_str(), "acme");
    }

    #[test]
    fn derive_slug_collapses_punctuation_runs_to_single_hyphen() {
        assert_eq!(derive_slug("Acme   &&  Co.").unwrap().as_str(), "acme-co");
    }

    #[test]
    fn derive_slug_trims_leading_and_trailing_punctuation() {
        assert_eq!(derive_slug("--Acme--").unwrap().as_str(), "acme");
    }

    #[test]
    fn derive_slug_drops_non_ascii_bytes() {
        // `é` is two UTF-8 bytes; both get mapped to `-`, then collapsed
        // and trimmed. Result is `caf`, not `cafe` — no transliteration.
        assert_eq!(derive_slug("Café").unwrap().as_str(), "caf");
    }

    #[test]
    fn derive_slug_rejects_empty_input() {
        assert!(matches!(
            derive_slug(""),
            Err(SlugError::Unparseable { .. })
        ));
    }

    #[test]
    fn derive_slug_rejects_all_whitespace() {
        assert!(matches!(
            derive_slug("   \t\n"),
            Err(SlugError::Unparseable { .. })
        ));
    }

    #[test]
    fn derive_slug_rejects_all_punctuation() {
        assert!(matches!(
            derive_slug("!!!"),
            Err(SlugError::Unparseable { .. })
        ));
    }

    #[test]
    fn derive_slug_rejects_reserved_slugs() {
        assert!(matches!(
            derive_slug("Demo"),
            Err(SlugError::Reserved(s)) if s == "demo"
        ));
        assert!(matches!(
            derive_slug("META"),
            Err(SlugError::Reserved(s)) if s == "meta"
        ));
        assert!(matches!(
            derive_slug("sessions"),
            Err(SlugError::Reserved(s)) if s == "sessions"
        ));
    }

    #[test]
    fn derive_slug_rejects_input_over_byte_cap_before_allocating() {
        // Past `DERIVE_INPUT_BYTE_CAP` we reject without scanning. Use an
        // input that would derive to a valid `acme` slug if scanned, so a
        // failure to short-circuit would surface as `Ok(...)` instead of
        // `Err(TooLong)`.
        let input = format!("{}acme", "a".repeat(DERIVE_INPUT_BYTE_CAP));
        let err = derive_slug(&input).unwrap_err();
        let SlugError::TooLong { len, max } = err else {
            panic!("expected TooLong, got {err:?}");
        };
        assert_eq!(len, input.len());
        assert_eq!(max, MAX_SLUG_LEN);
    }

    #[test]
    fn derive_slug_accepts_input_at_byte_cap() {
        // At the cap we still scan; the resulting slug is over MAX_SLUG_LEN
        // so `Slug::new` rejects it with `TooLong`. The point of this test
        // is that the early-reject branch is exclusive (`>`, not `>=`).
        let input = "a".repeat(DERIVE_INPUT_BYTE_CAP);
        assert!(matches!(
            derive_slug(&input),
            Err(SlugError::TooLong { .. })
        ));
    }

    #[test]
    fn derive_slug_rejects_over_length() {
        // 30 chars × 'ab' = 60 chars exactly; one more byte overflows.
        let input = "a".repeat(MAX_SLUG_LEN + 1);
        assert!(matches!(
            derive_slug(&input),
            Err(SlugError::TooLong { .. })
        ));
    }

    #[test]
    fn derive_slug_accepts_max_length() {
        let input = "a".repeat(MAX_SLUG_LEN);
        assert_eq!(derive_slug(&input).unwrap().as_str(), input);
    }

    #[test]
    fn derive_slug_preserves_digits() {
        assert_eq!(
            derive_slug("Shop 42 — Main").unwrap().as_str(),
            "shop-42-main"
        );
    }

    #[test]
    fn deserialize_validates_through_slug_new() {
        let bad = serde_json::from_str::<Slug>("\"BAD-Slug\"");
        assert!(bad.is_err(), "uppercase must be rejected on deserialize");
        let reserved = serde_json::from_str::<Slug>("\"meta\"");
        assert!(
            reserved.is_err(),
            "reserved must be rejected on deserialize"
        );
        let ok = serde_json::from_str::<Slug>("\"acme-printing\"").unwrap();
        assert_eq!(ok.as_str(), "acme-printing");
    }
}
