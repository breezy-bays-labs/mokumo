//! ADR frontmatter parser + walker.
//!
//! The schema is a deliberate subset of YAML; we hand-roll the extractor to
//! avoid pulling a YAML crate into `docs-gen`. Files without an opening
//! `---` line are silently skipped (legacy ADR format), so the gate is
//! dormant on un-migrated files and active on any file that opts into the
//! YAML convention.
//!
//! Recognized top-level keys: `title`, `status`, `enforced-by`. Other keys
//! are tolerated and ignored — the parser is not a schema validator for the
//! whole frontmatter, only for the enforcement contract. Unknown keys may
//! carry inline values (`tags: [a, b]`) or open block sequences/mappings
//! (`related:` followed by indented `- foo` lines, common in cross-link
//! frontmatter); both forms are skipped past without inspection.
//!
//! The block-sequence form for `enforced-by:` is the only form recognized:
//!
//! ```yaml
//! enforced-by:
//!   - kind: test
//!     ref: my::test::name
//!     note: Pins the foo invariant
//! ```
//!
//! Each item must carry `kind`, `ref`, and `note` scalars. Multi-line
//! scalars and flow syntax (`[{...}]`) are deliberately unsupported — keeping
//! the schema narrow makes both the parser and the audit trail predictable.

use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adr {
    pub path: PathBuf,
    pub title: String,
    pub status: String,
    pub enforced_by: Vec<EnforcedBy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnforcedBy {
    pub kind: EnforcedByKind,
    pub r#ref: String,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedByKind {
    Test,
    Lint,
    DepAbsence,
    Workflow,
    HumanJudgment,
}

impl EnforcedByKind {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "test" => Ok(Self::Test),
            "lint" => Ok(Self::Lint),
            "dep-absence" => Ok(Self::DepAbsence),
            "workflow" => Ok(Self::Workflow),
            "human-judgment" => Ok(Self::HumanJudgment),
            other => Err(anyhow!(
                "unknown enforced-by kind `{other}` (expected one of: \
                 test, lint, dep-absence, workflow, human-judgment)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Lint => "lint",
            Self::DepAbsence => "dep-absence",
            Self::Workflow => "workflow",
            Self::HumanJudgment => "human-judgment",
        }
    }
}

/// Returns `Ok(None)` for files without YAML frontmatter (legacy ADR
/// format). Returns `Err` for malformed YAML, unknown enforcement kinds,
/// or missing required keys inside an `enforced-by:` item.
pub fn parse_adr_file(path: &Path) -> Result<Option<Adr>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading ADR file {}", path.display()))?;
    parse_adr(&raw, path)
}

pub fn parse_adr(raw: &str, path: &Path) -> Result<Option<Adr>> {
    let Some(frontmatter) = extract_frontmatter(raw, path)? else {
        return Ok(None);
    };
    let TopLevel {
        title,
        status,
        enforced_by,
    } = walk_top_level(&frontmatter, path)?;
    Ok(Some(Adr {
        path: path.to_path_buf(),
        title: title.unwrap_or_else(|| derive_title_from_path(path)),
        status: status.unwrap_or_else(|| "unknown".to_string()),
        enforced_by: enforced_by.unwrap_or_default(),
    }))
}

/// Returns the lines between the opening and closing `---` markers, or
/// `Ok(None)` when the file has no opening `---` (legacy format). Errors
/// when the opening marker is present but never closes.
fn extract_frontmatter<'a>(raw: &'a str, path: &Path) -> Result<Option<Vec<&'a str>>> {
    let mut lines = raw.lines();
    let Some(first) = lines.next() else {
        return Ok(None);
    };
    if first.trim_end() != "---" {
        return Ok(None);
    }
    let mut frontmatter: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim_end() == "---" {
            return Ok(Some(frontmatter));
        }
        frontmatter.push(line);
    }
    bail!(
        "{}: YAML frontmatter started with `---` but never closed",
        path.display()
    );
}

#[derive(Default)]
struct TopLevel {
    title: Option<String>,
    status: Option<String>,
    enforced_by: Option<Vec<EnforcedBy>>,
}

/// Walks unindented `key: value` lines at the top of the frontmatter,
/// dispatching each to [`apply_one_top_level_line`].
fn walk_top_level(frontmatter: &[&str], path: &Path) -> Result<TopLevel> {
    let mut out = TopLevel::default();
    let mut i = 0;
    while i < frontmatter.len() {
        let consumed = apply_one_top_level_line(frontmatter, i, &mut out, path)?;
        i += consumed;
    }
    Ok(out)
}

/// Applies one frontmatter line, returning the number of lines consumed
/// (1 for ordinary scalar keys, more when `enforced-by:` opens a block
/// sequence). Unknown keys are tolerated; indented lines outside an
/// `enforced-by:` block are rejected so malformed files surface early.
fn apply_one_top_level_line(
    frontmatter: &[&str],
    i: usize,
    out: &mut TopLevel,
    path: &Path,
) -> Result<usize> {
    let line = frontmatter[i];
    if is_blank_or_comment(line) {
        return Ok(1);
    }
    if line.starts_with(' ') || line.starts_with('\t') {
        bail!(
            "{}: unexpected indented line at top of frontmatter: {:?}",
            path.display(),
            line
        );
    }
    let (key, value) = split_key_value(line).ok_or_else(|| {
        anyhow!(
            "{}: malformed frontmatter line (no `key: value`): {:?}",
            path.display(),
            line
        )
    })?;
    match key {
        "title" => out.title = Some(unquote(value).to_string()),
        "status" => out.status = Some(unquote(value).to_string()),
        "enforced-by" => return apply_enforced_by_opener(frontmatter, i, out, path, value),
        _ => {
            // Unknown keys with an inline value (`tags: [a, b]`) consume one
            // line. Unknown keys whose value is empty (`related:`) may open a
            // block sequence or block mapping; skip the indented continuation
            // lines so the next top-level key parses cleanly. The strict
            // schema for `enforced-by:` is unchanged — it routes through
            // `apply_enforced_by_opener` above, never reaching this arm.
            if value.is_empty() {
                return Ok(skip_indented_block(frontmatter, i));
            }
        }
    }
    Ok(1)
}

/// Returns the number of lines consumed from `i` covering an unknown-key
/// header plus any indented or blank continuation lines that follow. The
/// header at `i` is always counted; subsequent lines are consumed while they
/// either begin with whitespace or are blank/comment lines.
fn skip_indented_block(frontmatter: &[&str], i: usize) -> usize {
    let mut consumed = 1;
    while i + consumed < frontmatter.len() {
        let next = frontmatter[i + consumed];
        if next.starts_with(' ') || next.starts_with('\t') || is_blank_or_comment(next) {
            consumed += 1;
        } else {
            break;
        }
    }
    consumed
}

fn apply_enforced_by_opener(
    frontmatter: &[&str],
    i: usize,
    out: &mut TopLevel,
    path: &Path,
    value: &str,
) -> Result<usize> {
    if !value.trim().is_empty() {
        bail!(
            "{}: `enforced-by:` must open a block sequence; \
             found inline value: {:?}",
            path.display(),
            value
        );
    }
    let (items, consumed) = parse_enforced_by_block(&frontmatter[i + 1..], path)?;
    out.enforced_by = Some(items);
    Ok(consumed + 1)
}

fn is_blank_or_comment(line: &str) -> bool {
    line.trim().is_empty() || line.trim_start().starts_with('#')
}

/// Returns `(items, consumed_lines)` where `consumed_lines` is the count of
/// lines from `rest[..]` that the block sequence occupied — the caller
/// advances its cursor by that amount.
fn parse_enforced_by_block(rest: &[&str], path: &Path) -> Result<(Vec<EnforcedBy>, usize)> {
    let mut items: Vec<EnforcedBy> = Vec::new();
    let mut idx = 0;
    while idx < rest.len() {
        let line = rest[idx];
        if is_blank_or_comment(line) {
            idx += 1;
            continue;
        }
        // Block sequence items start with `  - ` (two spaces, hyphen,
        // space). Anything else either belongs to a subsequent top-level
        // key or is malformed.
        if !line.starts_with("  - ") {
            break;
        }
        let (item, item_consumed) = parse_one_block_item(&rest[idx..], path)?;
        items.push(item);
        idx += item_consumed;
    }
    if items.is_empty() {
        bail!(
            "{}: `enforced-by:` opened a block sequence but no items followed",
            path.display()
        );
    }
    Ok((items, idx))
}

/// Parses a single block-sequence item starting at `rest[0]` (which must
/// begin with `"  - "`). Returns the populated `EnforcedBy` and the number
/// of lines the item occupied (header + continuation lines).
fn parse_one_block_item(rest: &[&str], path: &Path) -> Result<(EnforcedBy, usize)> {
    let head = &rest[0][4..]; // strip "  - "
    let (k, v) = split_key_value(head).ok_or_else(|| {
        anyhow!(
            "{}: malformed enforced-by item header: {:?}",
            path.display(),
            rest[0]
        )
    })?;
    let mut fields = ItemFields::default();
    assign_item_field(k, v, &mut fields, path)?;
    let mut consumed = 1;
    while consumed < rest.len() {
        let cont = rest[consumed];
        if !cont.starts_with("    ") || cont.starts_with("    -") {
            break;
        }
        let (k, v) = split_key_value(cont.trim_start()).ok_or_else(|| {
            anyhow!(
                "{}: malformed enforced-by continuation line: {:?}",
                path.display(),
                cont
            )
        })?;
        assign_item_field(k, v, &mut fields, path)?;
        consumed += 1;
    }
    Ok((fields.into_enforced_by(path)?, consumed))
}

#[derive(Default)]
struct ItemFields {
    kind: Option<EnforcedByKind>,
    r#ref: Option<String>,
    note: Option<String>,
}

impl ItemFields {
    fn into_enforced_by(self, path: &Path) -> Result<EnforcedBy> {
        let kind = self.kind.ok_or_else(|| {
            anyhow!(
                "{}: enforced-by item missing required `kind` field",
                path.display()
            )
        })?;
        let r#ref = self.r#ref.ok_or_else(|| {
            anyhow!(
                "{}: enforced-by item missing required `ref` field",
                path.display()
            )
        })?;
        let note = self.note.ok_or_else(|| {
            anyhow!(
                "{}: enforced-by item missing required `note` field",
                path.display()
            )
        })?;
        Ok(EnforcedBy { kind, r#ref, note })
    }
}

fn assign_item_field(key: &str, value: &str, fields: &mut ItemFields, path: &Path) -> Result<()> {
    let value = unquote(value).to_string();
    match key {
        "kind" => fields.kind = Some(EnforcedByKind::parse(&value)?),
        "ref" => fields.r#ref = Some(value),
        "note" => fields.note = Some(value),
        other => bail!(
            "{}: unknown enforced-by item field `{other}` \
             (expected: kind, ref, note)",
            path.display()
        ),
    }
    Ok(())
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    let value = line[colon + 1..].trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

/// Strips a single layer of single or double quotes from the outside of
/// `value`. Internal escapes are not interpreted — callers are responsible
/// for keeping notes free of awkward characters, same as any plain YAML.
fn unquote(value: &str) -> &str {
    let v = value.trim();
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
        &v[1..v.len() - 1]
    } else {
        v
    }
}

fn derive_title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map_or_else(|| "untitled".to_string(), str::to_string)
}

/// Recursively walks `adr_root` for `*.md` files, parses each, and returns
/// the resulting `Adr` records sorted by `title` (ASCII-stable across
/// operating systems). Files without YAML frontmatter are skipped. The
/// recursion matches the lefthook + CI `docs/adr/**` glob so nested
/// directories are not silently dropped from the index.
pub fn walk_adrs(adr_root: &Path) -> Result<Vec<Adr>> {
    if !adr_root.exists() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<PathBuf> = Vec::new();
    collect_md_files(adr_root, &mut entries)?;
    entries.sort();
    let mut adrs: Vec<Adr> = Vec::new();
    for path in entries {
        if let Some(adr) = parse_adr_file(&path)? {
            adrs.push(adr);
        }
    }
    adrs.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(adrs)
}

fn collect_md_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("reading ADR dir {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_md_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn p() -> PathBuf {
        PathBuf::from("test.md")
    }

    #[test]
    fn parses_minimal_frontmatter_with_enforced_by() {
        let raw = "\
---
title: ADR-1: Foo
status: approved
enforced-by:
  - kind: test
    ref: tests::foo::bar
    note: Pins the foo invariant
---

# Body
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.title, "ADR-1: Foo");
        assert_eq!(adr.status, "approved");
        assert_eq!(adr.enforced_by.len(), 1);
        assert_eq!(adr.enforced_by[0].kind, EnforcedByKind::Test);
        assert_eq!(adr.enforced_by[0].r#ref, "tests::foo::bar");
        assert_eq!(adr.enforced_by[0].note, "Pins the foo invariant");
    }

    #[test]
    fn returns_none_for_legacy_format_without_yaml_frontmatter() {
        let raw = "# ADR: Coverage Exclusions\n\n**Status**: Accepted\n";
        assert!(parse_adr(raw, &p()).unwrap().is_none());
    }

    #[test]
    fn errors_on_unclosed_frontmatter() {
        let raw = "---\ntitle: Foo\n\nbody without closing\n";
        let err = parse_adr(raw, &p()).unwrap_err();
        assert!(err.to_string().contains("never closed"));
    }

    #[test]
    fn errors_on_unknown_enforced_by_kind() {
        let raw = "\
---
title: T
enforced-by:
  - kind: bogus
    ref: foo
    note: bar
---
";
        let err = parse_adr(raw, &p()).unwrap_err();
        assert!(err.to_string().contains("unknown enforced-by kind"));
    }

    #[test]
    fn errors_when_item_missing_ref() {
        let raw = "\
---
title: T
enforced-by:
  - kind: workflow
    note: bar
---
";
        let err = parse_adr(raw, &p()).unwrap_err();
        assert!(err.to_string().contains("missing required `ref` field"));
    }

    #[test]
    fn errors_when_enforced_by_block_sequence_is_empty() {
        let raw = "\
---
title: T
enforced-by:
other-key: x
---
";
        let err = parse_adr(raw, &p()).unwrap_err();
        assert!(err.to_string().contains("no items followed"));
    }

    #[test]
    fn parses_multiple_enforced_by_items() {
        let raw = "\
---
title: ADR
status: approved
enforced-by:
  - kind: test
    ref: x::y
    note: a
  - kind: workflow
    ref: .github/workflows/quality.yml
    note: b
  - kind: human-judgment
    ref: code review
    note: c
---
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.enforced_by.len(), 3);
        assert_eq!(adr.enforced_by[1].kind, EnforcedByKind::Workflow);
        assert_eq!(adr.enforced_by[2].kind, EnforcedByKind::HumanJudgment);
    }

    #[test]
    fn tolerates_unknown_top_level_keys() {
        let raw = "\
---
title: T
tags: [a, b]
created: 2026-05-03
status: approved
enforced-by:
  - kind: lint
    ref: scripts/check-foo.sh
    note: ok
---
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.title, "T");
    }

    #[test]
    fn tolerates_unknown_block_sequence_key() {
        // ADRs in the ops vault (e.g. adr-kikan-binary-topology.md) use
        // `related:` block sequences for cross-links. The validator must
        // skip past these without bailing on the indented continuation.
        let raw = "\
---
title: T
status: approved
related:
  - decisions/a.md
  - decisions/b.md
enforced-by:
  - kind: human-judgment
    ref: code review
    note: ok
---
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.title, "T");
        assert_eq!(adr.enforced_by.len(), 1);
    }

    #[test]
    fn tolerates_unknown_block_mapping_key() {
        // The same tolerance must extend to block mappings (e.g. `meta:`
        // with nested `author`/`date` scalars), not just block sequences.
        let raw = "\
---
title: T
status: approved
meta:
  author: alice
  date: 2026-05-04
enforced-by:
  - kind: human-judgment
    ref: code review
    note: ok
---
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.title, "T");
        assert_eq!(adr.enforced_by.len(), 1);
    }

    #[test]
    fn enforced_by_strictness_unchanged_under_unknown_key_tolerance() {
        // Regression guard: relaxing unknown-key handling must not weaken
        // the `enforced-by:` schema. Malformed items still bail.
        let raw = "\
---
title: T
related:
  - foo
enforced-by:
  - kind: bogus
    ref: x
    note: y
---
";
        let err = parse_adr(raw, &p()).unwrap_err();
        assert!(err.to_string().contains("unknown enforced-by kind"));
    }

    #[test]
    fn handles_quoted_scalars() {
        let raw = "\
---
title: \"ADR: With colon\"
status: 'approved'
enforced-by:
  - kind: test
    ref: \"a::b\"
    note: 'a note'
---
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.title, "ADR: With colon");
        assert_eq!(adr.status, "approved");
        assert_eq!(adr.enforced_by[0].r#ref, "a::b");
        assert_eq!(adr.enforced_by[0].note, "a note");
    }

    #[test]
    fn errors_on_indented_top_level_line() {
        let raw = "\
---
title: T
  rogue-indent: oops
---
";
        let err = parse_adr(raw, &p()).unwrap_err();
        assert!(err.to_string().contains("unexpected indented line"));
    }

    #[test]
    fn errors_on_malformed_frontmatter_line_without_colon() {
        let raw = "\
---
title: T
this-line-has-no-colon
---
";
        let err = parse_adr(raw, &p()).unwrap_err();
        assert!(
            err.to_string().contains("malformed frontmatter line"),
            "got: {err}"
        );
    }

    #[test]
    fn errors_on_inline_value_for_enforced_by() {
        let raw = "\
---
title: T
enforced-by: not-a-block-sequence
---
";
        let err = parse_adr(raw, &p()).unwrap_err();
        assert!(err.to_string().contains("must open a block sequence"));
    }

    #[test]
    fn tolerates_blank_lines_and_comments_in_frontmatter() {
        let raw = "\
---
# leading comment
title: T

# blank line above and comment here
status: approved
enforced-by:
  - kind: test
    ref: a
    note: b
---
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.title, "T");
        assert_eq!(adr.status, "approved");
        assert_eq!(adr.enforced_by.len(), 1);
    }

    #[test]
    fn enforced_by_block_tolerates_comment_lines() {
        let raw = "\
---
title: T
enforced-by:
  # first item below
  - kind: test
    ref: a
    note: b

  # second item
  - kind: workflow
    ref: .github/workflows/quality.yml
    note: c
---
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.enforced_by.len(), 2);
    }

    #[test]
    fn derives_title_from_filename_when_title_key_absent() {
        let raw = "\
---
status: approved
enforced-by:
  - kind: test
    ref: a
    note: b
---
";
        let path = std::path::PathBuf::from("/tmp/adr-foo-bar.md");
        let adr = parse_adr(raw, &path).unwrap().unwrap();
        assert_eq!(adr.title, "adr-foo-bar");
    }

    #[test]
    fn frontmatter_without_enforced_by_yields_unmanaged_adr() {
        // The parser is descriptive: a YAML-frontmatter ADR without an
        // `enforced-by:` key parses successfully with an empty enforcement
        // list. The CI gate is what catches the missing contract on touch.
        let raw = "\
---
title: Old-style entry
status: approved
---
";
        let adr = parse_adr(raw, &p()).unwrap().unwrap();
        assert_eq!(adr.title, "Old-style entry");
        assert!(adr.enforced_by.is_empty());
    }

    #[test]
    fn walk_returns_empty_when_root_missing() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(walk_adrs(&missing).unwrap().is_empty());
    }

    #[test]
    fn walk_recurses_into_subdirectories() {
        // Lefthook + CI globs use `docs/adr/**`; the walker must match.
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested/deeper");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            dir.path().join("top.md"),
            "\
---
title: Top
status: approved
enforced-by:
  - kind: test
    ref: a
    note: b
---
",
        )
        .unwrap();
        std::fs::write(
            nested.join("deep.md"),
            "\
---
title: Deep
status: approved
enforced-by:
  - kind: test
    ref: a
    note: b
---
",
        )
        .unwrap();
        let adrs = walk_adrs(dir.path()).unwrap();
        let titles: Vec<&str> = adrs.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, vec!["Deep", "Top"]);
    }

    #[test]
    fn walk_skips_legacy_files_and_sorts_by_title() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("z-newer.md"),
            "\
---
title: Aardvark
status: approved
enforced-by:
  - kind: test
    ref: x
    note: y
---
",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("a-older.md"),
            "\
---
title: Beetle
status: approved
enforced-by:
  - kind: test
    ref: x
    note: y
---
",
        )
        .unwrap();
        std::fs::write(dir.path().join("legacy.md"), "# Old format\n").unwrap();
        let adrs = walk_adrs(dir.path()).unwrap();
        assert_eq!(adrs.len(), 2);
        // Sorted by title, so "Aardvark" precedes "Beetle".
        assert_eq!(adrs[0].title, "Aardvark");
        assert_eq!(adrs[1].title, "Beetle");
    }
}
