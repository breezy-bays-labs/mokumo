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
