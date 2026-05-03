//! Workspace-root discovery.
//!
//! Walks up from the current working directory until it finds a
//! `Cargo.toml` containing a `[workspace]` table. Lets the binary work
//! regardless of where the user invokes it from (Moon, lefthook, CI, or
//! a developer's shell sitting in a subdirectory).

use anyhow::{Result, bail};
use std::path::PathBuf;

pub fn find_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let raw = std::fs::read_to_string(&cargo_toml)?;
            // `[workspace]` opens the table; `[workspace.package]` /
            // `[workspace.dependencies]` are sub-tables — both pin this
            // file as a workspace root.
            if raw.contains("[workspace]") || raw.contains("[workspace.") {
                return Ok(dir);
            }
        }
        if !dir.pop() {
            bail!("could not find a workspace Cargo.toml in any ancestor of the current directory");
        }
    }
}
