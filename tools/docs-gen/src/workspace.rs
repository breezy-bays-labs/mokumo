//! Workspace-root discovery.
//!
//! Walks up from a starting directory until it finds a `Cargo.toml`
//! containing a `[workspace]` table. Lets the binary work regardless of
//! where the user invokes it from (Moon, lefthook, CI, or a developer's
//! shell sitting in a subdirectory).

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub fn find_root() -> Result<PathBuf> {
    let start = std::env::current_dir()?;
    find_root_from(&start)
}

/// Walks up from `start`. Extracted from [`find_root`] so tests can pin a
/// known starting directory inside a tempdir.
pub fn find_root_from(start: &Path) -> Result<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists()
            && let Ok(raw) = std::fs::read_to_string(&cargo_toml)
            // `[workspace]` opens the table; `[workspace.package]` /
            // `[workspace.dependencies]` are sub-tables — both pin this
            // file as a workspace root.
            && (raw.contains("[workspace]") || raw.contains("[workspace."))
        {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!(
                "could not find a workspace Cargo.toml in any ancestor of {}",
                start.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &Path, body: &str) {
        std::fs::write(p, body).unwrap();
    }

    /// Two tempdir paths can differ when one is canonicalized through a
    /// symlink (e.g. macOS `/var/folders/...` ↔ `/private/var/...`).
    fn assert_same_path(a: &Path, b: &Path) {
        assert_eq!(a.canonicalize().unwrap(), b.canonicalize().unwrap());
    }

    #[test]
    fn returns_start_when_it_is_the_workspace_root() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n",
        );
        let found = find_root_from(dir.path()).unwrap();
        assert_same_path(&found, dir.path());
    }

    #[test]
    fn walks_up_from_a_subdirectory() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n",
        );
        let nested = dir.path().join("crates").join("foo");
        std::fs::create_dir_all(&nested).unwrap();
        let found = find_root_from(&nested).unwrap();
        assert_same_path(&found, dir.path());
    }

    #[test]
    fn skips_member_cargo_toml_without_workspace_table() {
        // Member crates have only `[package]`. The walker must not stop
        // there — it has to keep climbing until it sees `[workspace]`.
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\n",
        );
        let member = dir.path().join("member");
        std::fs::create_dir_all(&member).unwrap();
        write(&member.join("Cargo.toml"), "[package]\nname = \"member\"\n");
        let found = find_root_from(&member).unwrap();
        assert_same_path(&found, dir.path());
    }

    #[test]
    fn recognizes_bare_workspace_subtable() {
        // A Cargo.toml with only `[workspace.package]` (no `[workspace]`)
        // is unusual but `find_root_from` accepts it via the substring
        // branch.
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            "[workspace.package]\nrust-version = \"1.95\"\n",
        );
        let found = find_root_from(dir.path()).unwrap();
        assert_same_path(&found, dir.path());
    }

    #[test]
    fn errors_when_no_ancestor_has_workspace_cargo_toml() {
        // tempdir lives under the OS temp dir (`/tmp`, `$TMPDIR`, etc.);
        // none of those have a `[workspace]` Cargo.toml, so the walk
        // must terminate with `bail!`.
        let dir = tempdir().unwrap();
        let err = find_root_from(dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("could not find"),
            "expected bail message, got: {err}"
        );
    }

    #[test]
    fn ignores_unreadable_cargo_toml_and_keeps_walking() {
        // Empty / unparseable Cargo.toml shouldn't anchor the walk.
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n",
        );
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        write(&nested.join("Cargo.toml"), ""); // empty — no workspace markers
        let found = find_root_from(&nested).unwrap();
        assert_same_path(&found, dir.path());
    }
}
