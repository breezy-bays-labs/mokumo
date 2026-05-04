//! Read crate exclusions from `crap4rs.toml` so the per-handler producer
//! shares the same exclusion ledger as the CRAP gate.
//!
//! The two tools target the same surface (Mokumo's HTTP handlers) but
//! through different lenses (CRAP for change risk, branch coverage for
//! negative-path detection). Maintaining a single source of truth for
//! "what's intentionally outside scope" means an operator excluding a
//! crate from CRAP automatically excludes it from the per-handler
//! breakouts — and vice versa, surfacing a regression from one config
//! drift instead of two.
//!
//! Format excerpt of `crap4rs.toml`:
//!
//! ```toml
//! [exclusions]
//! crates = ["mokumo-desktop", "kikan-tauri", "kikan-admin-ui"]
//! ```
//!
//! Crate names are read in their package form (hyphenated). Callers
//! converting to the Rust-ident form (`mokumo_desktop`) should do so via
//! [`to_ident`] so the two views stay aligned.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Default, Deserialize)]
struct Crap4rsToml {
    #[serde(default)]
    exclusions: Exclusions,
}

#[derive(Debug, Default, Deserialize)]
struct Exclusions {
    #[serde(default)]
    crates: Vec<String>,
}

/// Excluded crate names in **package form** (hyphenated, e.g.
/// `"mokumo-desktop"`) — the same form the operator wrote in
/// `crap4rs.toml`.
#[derive(Debug, Clone, Default)]
pub struct ExcludedCrates {
    set: HashSet<String>,
}

impl ExcludedCrates {
    /// Read `crap4rs.toml` from the workspace root. Missing file is OK
    /// (treated as empty exclusion set) — the producer should not require
    /// a CRAP config to run.
    pub fn read(workspace_root: &Path) -> Result<Self> {
        let path = workspace_root.join("crap4rs.toml");
        if !path.is_file() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let parsed: Crap4rsToml =
            toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Self {
            set: parsed.exclusions.crates.into_iter().collect(),
        })
    }

    /// Construct directly from a list of package names (test seam).
    /// Named `from_packages` rather than `from_iter` to avoid clippy's
    /// `should_implement_trait` flag for the `FromIterator::from_iter`
    /// signature collision.
    #[must_use]
    pub fn from_packages<I, S>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            set: iter.into_iter().map(Into::into).collect(),
        }
    }

    /// Test in package form (`"mokumo-desktop"`).
    #[must_use]
    pub fn contains_package(&self, package_name: &str) -> bool {
        self.set.contains(package_name)
    }

    /// Sorted list of excluded packages — for deterministic diagnostic output.
    #[must_use]
    pub fn sorted_packages(&self) -> Vec<String> {
        let mut v: Vec<_> = self.set.iter().cloned().collect();
        v.sort();
        v
    }
}

/// Convert a package name to its Rust-ident form (hyphens → underscores).
#[must_use]
pub fn to_ident(package_name: &str) -> String {
    package_name.replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_yields_empty_set() {
        let tmp = tempdir().unwrap();
        let ex = ExcludedCrates::read(tmp.path()).unwrap();
        assert!(!ex.contains_package("anything"));
    }

    #[test]
    fn reads_crates_list() {
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("crap4rs.toml"),
            r#"[exclusions]
crates = ["mokumo-desktop", "kikan-tauri"]"#,
        )
        .unwrap();
        let ex = ExcludedCrates::read(tmp.path()).unwrap();
        assert!(ex.contains_package("mokumo-desktop"));
        assert!(ex.contains_package("kikan-tauri"));
        assert!(!ex.contains_package("kikan"));
    }

    #[test]
    fn ident_form_swaps_hyphens() {
        assert_eq!(to_ident("mokumo-desktop"), "mokumo_desktop");
        assert_eq!(to_ident("kikan"), "kikan");
    }
}
