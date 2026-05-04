//! Registry of every `<!-- AUTO-GEN:* -->` region this binary owns.
//!
//! Adding a new section is two changes: write a `render_*` function that
//! produces the markdown body, then append a [`Section`] to [`all`]. The
//! corresponding marker pair must already exist in the target file.
//!
//! Renderers receive the absolute workspace root so they can read source
//! files (`Cargo.toml`, coverage reports, etc.) without re-discovering it.

use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;

use crate::{adr, badge, msrv};

pub struct Section {
    pub name: &'static str,
    /// Workspace-relative path of the file this section lives in.
    pub target: &'static str,
    pub render: fn(&Path) -> Result<String>,
}

pub fn all() -> Vec<Section> {
    vec![
        Section {
            name: "msrv",
            target: "README.md",
            render: render_msrv,
        },
        Section {
            name: "adr-index",
            target: "docs/adr-index.md",
            render: render_adr_index,
        },
    ]
}

fn render_msrv(workspace_root: &Path) -> Result<String> {
    let version = msrv::read(workspace_root)?;
    let url = badge::static_url("MSRV", &version, "blue");
    Ok(format!("[![MSRV]({url})](Cargo.toml)"))
}

/// Renders the ADR registry table. Walks `docs/adr/` for files with YAML
/// frontmatter, sorts by title (set in [`adr::walk_adrs`]), and emits one
/// row per ADR with a deduplicated, alphabetized list of enforcement kinds.
/// Empty input renders a placeholder so the marker pair stays load-bearing
/// before any ADR opts in.
fn render_adr_index(workspace_root: &Path) -> Result<String> {
    let adrs = adr::walk_adrs(&workspace_root.join("docs/adr"))?;
    if adrs.is_empty() {
        return Ok("_No ADRs registered yet._".to_string());
    }
    let mut out = String::from("| Title | Status | Source | Enforcement |\n|---|---|---|---|\n");
    for entry in adrs {
        let rel = entry
            .path
            .strip_prefix(workspace_root)
            .unwrap_or(&entry.path)
            .display()
            .to_string();
        // Sort + dedupe enforcement kinds for stable output. A row with two
        // `kind: test` items still reads as "test"; that matches what the
        // operator wants in a dashboard view.
        let mut kinds: BTreeSet<&'static str> = BTreeSet::new();
        for ev in &entry.enforced_by {
            kinds.insert(ev.kind.as_str());
        }
        let enforcement = if kinds.is_empty() {
            "_unmanaged_".to_string()
        } else {
            kinds.into_iter().collect::<Vec<_>>().join(", ")
        };
        out.push_str(&format!(
            "| {title} | {status} | [{rel}]({rel}) | {enforcement} |\n",
            title = entry.title.replace('|', "\\|"),
            status = entry.status,
            rel = rel,
            enforcement = enforcement,
        ));
    }
    // Trim the trailing newline so `markers::rewrite` produces fixed-point
    // output (it adds its own surrounding newlines).
    if out.ends_with('\n') {
        out.pop();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn render_msrv_produces_expected_badge() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace.package]\nrust-version = \"1.95\"\n",
        )
        .unwrap();
        let out = render_msrv(dir.path()).unwrap();
        assert_eq!(
            out,
            "[![MSRV](https://img.shields.io/badge/MSRV-1.95-blue.svg)](Cargo.toml)"
        );
    }

    #[test]
    fn render_adr_index_returns_placeholder_when_empty() {
        let dir = tempdir().unwrap();
        // No `docs/adr/` directory at all — empty-set is valid.
        let out = render_adr_index(dir.path()).unwrap();
        assert_eq!(out, "_No ADRs registered yet._");
    }

    #[test]
    fn render_adr_index_emits_table_sorted_by_title() {
        let dir = tempdir().unwrap();
        let adr_dir = dir.path().join("docs/adr");
        std::fs::create_dir_all(&adr_dir).unwrap();
        std::fs::write(
            adr_dir.join("z-first.md"),
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
            adr_dir.join("a-second.md"),
            "\
---
title: Beetle
status: draft
enforced-by:
  - kind: workflow
    ref: .github/workflows/quality.yml
    note: y
  - kind: test
    ref: q::r
    note: y
---
",
        )
        .unwrap();
        let out = render_adr_index(dir.path()).unwrap();
        let aardvark_pos = out.find("Aardvark").expect("Aardvark row");
        let beetle_pos = out.find("Beetle").expect("Beetle row");
        assert!(
            aardvark_pos < beetle_pos,
            "rows must be alphabetized by title"
        );
        // Beetle row dedupes kinds and sorts: "test, workflow"
        assert!(out.contains("test, workflow"));
        // Aardvark row has only `test`.
        assert!(out.contains("| Aardvark | approved | "));
        // No trailing newline (markers::rewrite adds its own).
        assert!(!out.ends_with('\n'));
    }

    #[test]
    fn render_adr_index_escapes_pipes_in_titles() {
        let dir = tempdir().unwrap();
        let adr_dir = dir.path().join("docs/adr");
        std::fs::create_dir_all(&adr_dir).unwrap();
        std::fs::write(
            adr_dir.join("a.md"),
            "\
---
title: \"A | B title\"
status: approved
enforced-by:
  - kind: test
    ref: x
    note: y
---
",
        )
        .unwrap();
        let out = render_adr_index(dir.path()).unwrap();
        assert!(out.contains("A \\| B title"));
    }
}
