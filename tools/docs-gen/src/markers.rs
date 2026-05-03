//! Marker parsing and replacement for `<!-- AUTO-GEN:name -->` regions.

use anyhow::{Result, anyhow};

/// Replaces the body between `<!-- AUTO-GEN:{section} -->` and
/// `<!-- /AUTO-GEN:{section} -->` in `content` with `body`.
///
/// Errors if either marker is missing, or if a second opening marker for
/// the same section appears before the closing marker (malformed nesting).
/// The replacement is wrapped in newlines so re-running with the same body
/// is a fixed point.
pub fn rewrite(content: &str, section: &str, body: &str) -> Result<String> {
    let open = format!("<!-- AUTO-GEN:{section} -->");
    let close = format!("<!-- /AUTO-GEN:{section} -->");

    let open_idx = content
        .find(&open)
        .ok_or_else(|| anyhow!("opening marker `{open}` missing"))?;
    let after_open = open_idx + open.len();

    let close_rel = content[after_open..]
        .find(&close)
        .ok_or_else(|| anyhow!("closing marker `{close}` missing"))?;
    let close_idx = after_open + close_rel;

    if content[after_open..close_idx].contains(&open) {
        return Err(anyhow!(
            "duplicate opening marker for section `{section}` before its close"
        ));
    }

    let mut out = String::with_capacity(content.len() + body.len());
    out.push_str(&content[..after_open]);
    out.push('\n');
    out.push_str(body);
    out.push('\n');
    out.push_str(&content[close_idx..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_section() {
        let input = "before\n<!-- AUTO-GEN:msrv -->\nold\n<!-- /AUTO-GEN:msrv -->\nafter\n";
        let out = rewrite(input, "msrv", "NEW").unwrap();
        assert_eq!(
            out,
            "before\n<!-- AUTO-GEN:msrv -->\nNEW\n<!-- /AUTO-GEN:msrv -->\nafter\n"
        );
    }

    #[test]
    fn idempotent_when_body_unchanged() {
        let input = "<!-- AUTO-GEN:x -->\nold\n<!-- /AUTO-GEN:x -->\n";
        let once = rewrite(input, "x", "Y").unwrap();
        let twice = rewrite(&once, "x", "Y").unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn errors_on_missing_open() {
        let err = rewrite("no markers", "msrv", "x").unwrap_err();
        assert!(err.to_string().contains("opening marker"));
    }

    #[test]
    fn errors_on_missing_close() {
        let err = rewrite("<!-- AUTO-GEN:msrv -->\n", "msrv", "x").unwrap_err();
        assert!(err.to_string().contains("closing marker"));
    }

    #[test]
    fn touches_only_named_section() {
        let input = "\
<!-- AUTO-GEN:a -->
A old
<!-- /AUTO-GEN:a -->
<!-- AUTO-GEN:b -->
B old
<!-- /AUTO-GEN:b -->
";
        let out = rewrite(input, "a", "A NEW").unwrap();
        assert!(out.contains("A NEW"));
        assert!(out.contains("B old"));
        assert!(!out.contains("A old"));
    }

    #[test]
    fn errors_on_duplicate_open() {
        let input = "<!-- AUTO-GEN:x -->\n<!-- AUTO-GEN:x -->\n<!-- /AUTO-GEN:x -->\n";
        let err = rewrite(input, "x", "y").unwrap_err();
        assert!(err.to_string().contains("duplicate opening marker"));
    }

    #[test]
    fn distinct_section_close_does_not_match() {
        // <!-- /AUTO-GEN:other --> appearing inside an `x` region must not
        // be treated as `x`'s close marker.
        let input =
            "<!-- AUTO-GEN:x -->\nbody with <!-- /AUTO-GEN:other -->\n<!-- /AUTO-GEN:x -->\n";
        let out = rewrite(input, "x", "Z").unwrap();
        assert!(out.contains("\nZ\n"));
    }
}
