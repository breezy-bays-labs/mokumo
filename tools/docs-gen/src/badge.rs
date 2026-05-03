//! shields.io static-badge URL helpers.
//!
//! shields.io requires double-escaping of `-`, `_`, and ` ` in label and
//! message components. Without it, a value like `nightly-2025-01-15` would
//! render as a malformed three-segment badge.

/// Escapes a label or message component per shields.io rules.
pub fn shields_escape(s: &str) -> String {
    s.replace('-', "--").replace('_', "__").replace(' ', "_")
}

/// Builds a static-badge URL: `https://img.shields.io/badge/{label}-{message}-{color}.svg`.
pub fn static_url(label: &str, message: &str, color: &str) -> String {
    format!(
        "https://img.shields.io/badge/{}-{}-{}.svg",
        shields_escape(label),
        shields_escape(message),
        color
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_clean_input() {
        assert_eq!(shields_escape("1.95"), "1.95");
        assert_eq!(shields_escape("MSRV"), "MSRV");
    }

    #[test]
    fn doubles_hyphens() {
        assert_eq!(
            shields_escape("nightly-2025-01-15"),
            "nightly--2025--01--15"
        );
    }

    #[test]
    fn doubles_underscores() {
        assert_eq!(shields_escape("a_b"), "a__b");
    }

    #[test]
    fn underscores_spaces() {
        assert_eq!(shields_escape("hello world"), "hello_world");
    }

    #[test]
    fn builds_msrv_url() {
        assert_eq!(
            static_url("MSRV", "1.95", "blue"),
            "https://img.shields.io/badge/MSRV-1.95-blue.svg"
        );
    }

    #[test]
    fn builds_url_with_nightly_msrv() {
        assert_eq!(
            static_url("MSRV", "nightly-2025-01-15", "blue"),
            "https://img.shields.io/badge/MSRV-nightly--2025--01--15-blue.svg"
        );
    }
}
