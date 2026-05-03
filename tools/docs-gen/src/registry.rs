//! Registry of every `<!-- AUTO-GEN:* -->` region this binary owns.
//!
//! Adding a new section is two changes: write a `render_*` function that
//! produces the markdown body, then append a [`Section`] to [`all`]. The
//! corresponding marker pair must already exist in the target file.
//!
//! Renderers receive the absolute workspace root so they can read source
//! files (`Cargo.toml`, coverage reports, etc.) without re-discovering it.

use anyhow::Result;
use std::path::Path;

use crate::{badge, msrv};

pub struct Section {
    pub name: &'static str,
    /// Workspace-relative path of the file this section lives in.
    pub target: &'static str,
    pub render: fn(&Path) -> Result<String>,
}

pub fn all() -> Vec<Section> {
    vec![Section {
        name: "msrv",
        target: "README.md",
        render: render_msrv,
    }]
}

fn render_msrv(workspace_root: &Path) -> Result<String> {
    let version = msrv::read(workspace_root)?;
    let url = badge::static_url("MSRV", &version, "blue");
    Ok(format!("[![MSRV]({url})](Cargo.toml)"))
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
}
