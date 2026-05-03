//! Codified-docs generator.
//!
//! Each entry in [`registry::all`] declares an `<!-- AUTO-GEN:name -->` /
//! `<!-- /AUTO-GEN:name -->` region in a target file and a renderer that
//! produces its body. [`run`] reads each target, rewrites every owned
//! region in place, and writes back only when the content changed.
//!
//! The drift gate is external: CI invokes the binary, then asserts
//! `git diff --exit-code` against the target files. Determinism is the
//! load-bearing invariant — renderers must produce byte-identical output
//! given identical source inputs (no timestamps, no path variation, no
//! map-iteration order leaking into output).

pub mod badge;
pub mod markers;
pub mod msrv;
pub mod registry;
pub mod workspace;

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

use crate::registry::Section;

/// Regenerates every section in `sections`. Each target file is read once,
/// rewritten in memory for all of its sections, and written back only if
/// the content actually changed (avoids gratuitous mtime churn).
pub fn run(workspace_root: &Path, sections: &[Section]) -> Result<()> {
    // BTreeMap for deterministic iteration order across runs.
    let mut by_target: BTreeMap<&str, Vec<&Section>> = BTreeMap::new();
    for s in sections {
        by_target.entry(s.target).or_default().push(s);
    }

    for (target, secs) in by_target {
        let abs = workspace_root.join(target);
        let original =
            std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;
        let mut content = original.clone();
        for sec in secs {
            let body = (sec.render)(workspace_root)
                .with_context(|| format!("rendering section `{}`", sec.name))?;
            content = markers::rewrite(&content, sec.name, &body)
                .with_context(|| format!("rewriting `{}` in {}", sec.name, target))?;
        }
        if content == original {
            eprintln!("docs-gen: {target} unchanged");
        } else {
            std::fs::write(&abs, &content).with_context(|| format!("writing {}", abs.display()))?;
            eprintln!("docs-gen: rewrote {target}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn render_static_x(_: &Path) -> anyhow::Result<String> {
        Ok("X CONTENT".to_string())
    }
    fn render_static_y(_: &Path) -> anyhow::Result<String> {
        Ok("Y CONTENT".to_string())
    }
    fn render_constant_same(_: &Path) -> anyhow::Result<String> {
        Ok("SAME".to_string())
    }
    fn render_failing(_: &Path) -> anyhow::Result<String> {
        Err(anyhow::anyhow!("renderer-deliberate-failure"))
    }

    fn write(p: &Path, body: &str) {
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn rewrites_target_when_body_differs() {
        let root = tempdir().unwrap();
        write(
            &root.path().join("FOO.md"),
            "<!-- AUTO-GEN:x -->\nold\n<!-- /AUTO-GEN:x -->\n",
        );
        let sections = vec![Section {
            name: "x",
            target: "FOO.md",
            render: render_static_x,
        }];
        run(root.path(), &sections).unwrap();
        let after = std::fs::read_to_string(root.path().join("FOO.md")).unwrap();
        assert!(after.contains("X CONTENT"));
        assert!(!after.contains("old"));
    }

    #[test]
    fn skips_write_when_content_unchanged() {
        // The `if content == original` branch must avoid touching the
        // file — important for editor watchers and `git status`.
        let root = tempdir().unwrap();
        let target = root.path().join("FOO.md");
        write(&target, "<!-- AUTO-GEN:x -->\nSAME\n<!-- /AUTO-GEN:x -->\n");
        let mtime_before = std::fs::metadata(&target).unwrap().modified().unwrap();
        // Sleep long enough that any rewrite would advance mtime past
        // filesystem timestamp resolution (HFS+ is 1s, ext4 is ~ns).
        std::thread::sleep(std::time::Duration::from_millis(20));
        let sections = vec![Section {
            name: "x",
            target: "FOO.md",
            render: render_constant_same,
        }];
        run(root.path(), &sections).unwrap();
        let mtime_after = std::fs::metadata(&target).unwrap().modified().unwrap();
        assert_eq!(mtime_before, mtime_after, "no-op run must not write");
    }

    #[test]
    fn errors_when_target_missing() {
        let root = tempdir().unwrap();
        let sections = vec![Section {
            name: "x",
            target: "DOES_NOT_EXIST.md",
            render: render_static_x,
        }];
        let err = run(root.path(), &sections).unwrap_err();
        assert!(
            err.to_string().contains("reading"),
            "expected reading-context error, got: {err}"
        );
    }

    #[test]
    fn errors_when_marker_missing_in_target() {
        let root = tempdir().unwrap();
        write(&root.path().join("FOO.md"), "no markers here\n");
        let sections = vec![Section {
            name: "x",
            target: "FOO.md",
            render: render_static_x,
        }];
        let err = run(root.path(), &sections).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("rewriting"),
            "expected rewriting-context error, got: {msg}"
        );
    }

    #[test]
    fn errors_when_renderer_fails() {
        let root = tempdir().unwrap();
        write(
            &root.path().join("FOO.md"),
            "<!-- AUTO-GEN:x -->\n\n<!-- /AUTO-GEN:x -->\n",
        );
        let sections = vec![Section {
            name: "x",
            target: "FOO.md",
            render: render_failing,
        }];
        let err = run(root.path(), &sections).unwrap_err();
        assert!(
            err.to_string().contains("rendering"),
            "expected rendering-context error, got: {err}"
        );
    }

    #[test]
    fn applies_multiple_sections_to_one_target() {
        let root = tempdir().unwrap();
        write(
            &root.path().join("FOO.md"),
            "<!-- AUTO-GEN:a -->\nA\n<!-- /AUTO-GEN:a -->\n\
             <!-- AUTO-GEN:b -->\nB\n<!-- /AUTO-GEN:b -->\n",
        );
        let sections = vec![
            Section {
                name: "a",
                target: "FOO.md",
                render: render_static_x,
            },
            Section {
                name: "b",
                target: "FOO.md",
                render: render_static_y,
            },
        ];
        run(root.path(), &sections).unwrap();
        let after = std::fs::read_to_string(root.path().join("FOO.md")).unwrap();
        assert!(after.contains("X CONTENT"));
        assert!(after.contains("Y CONTENT"));
    }

    #[test]
    fn applies_sections_across_multiple_targets() {
        let root = tempdir().unwrap();
        write(
            &root.path().join("A.md"),
            "<!-- AUTO-GEN:x -->\n\n<!-- /AUTO-GEN:x -->\n",
        );
        write(
            &root.path().join("B.md"),
            "<!-- AUTO-GEN:y -->\n\n<!-- /AUTO-GEN:y -->\n",
        );
        let sections = vec![
            Section {
                name: "x",
                target: "A.md",
                render: render_static_x,
            },
            Section {
                name: "y",
                target: "B.md",
                render: render_static_y,
            },
        ];
        run(root.path(), &sections).unwrap();
        let a = std::fs::read_to_string(root.path().join("A.md")).unwrap();
        let b = std::fs::read_to_string(root.path().join("B.md")).unwrap();
        assert!(a.contains("X CONTENT"));
        assert!(b.contains("Y CONTENT"));
    }

    #[test]
    fn empty_section_list_is_a_noop() {
        let root = tempdir().unwrap();
        // No targets, no work — must not panic or error.
        run(root.path(), &[]).unwrap();
    }
}
