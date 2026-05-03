//! MSRV (minimum supported Rust version) extraction from `Cargo.toml`.
//!
//! Reads `workspace.package.rust-version` — the canonical, Cargo-blessed
//! MSRV declaration. This is intentionally NOT `rust-toolchain.toml`'s
//! `channel` field, which pins the toolchain rustup downloads to build
//! the project (often more specific than the MSRV, e.g. `1.95.0` vs `1.95`,
//! and pinned for trybuild snapshot stability rather than MSRV semantics).

use anyhow::{Context, Result, anyhow};
use std::path::Path;

pub fn read(workspace_root: &Path) -> Result<String> {
    let path = workspace_root.join("Cargo.toml");
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&raw).with_context(|| format!("parsing {} as TOML", path.display()))?;

    value
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("rust-version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            anyhow!(
                "workspace.package.rust-version not found in {}",
                path.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_cargo(dir: &Path, body: &str) {
        std::fs::write(dir.join("Cargo.toml"), body).unwrap();
    }

    #[test]
    fn reads_rust_version() {
        let dir = tempdir().unwrap();
        write_cargo(dir.path(), "[workspace.package]\nrust-version = \"1.95\"\n");
        assert_eq!(read(dir.path()).unwrap(), "1.95");
    }

    #[test]
    fn errors_when_missing() {
        let dir = tempdir().unwrap();
        write_cargo(dir.path(), "[workspace.package]\nedition = \"2024\"\n");
        let err = read(dir.path()).unwrap_err();
        assert!(err.to_string().contains("rust-version"));
    }

    #[test]
    fn errors_when_cargo_toml_missing() {
        let dir = tempdir().unwrap();
        let err = read(dir.path()).unwrap_err();
        assert!(err.to_string().contains("reading"));
    }

    #[test]
    fn ignores_package_rust_version_outside_workspace_table() {
        // A non-workspace `[package]` rust-version must not be picked up —
        // the registry contract is "workspace MSRV", not member MSRV.
        let dir = tempdir().unwrap();
        write_cargo(
            dir.path(),
            "[package]\nname = \"foo\"\nrust-version = \"1.70\"\n",
        );
        let err = read(dir.path()).unwrap_err();
        assert!(err.to_string().contains("workspace.package.rust-version"));
    }
}
