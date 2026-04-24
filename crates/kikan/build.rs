//! Emit a short git SHA as `KIKAN_ENGINE_COMMIT` for
//! `kikan::data_plane::kikan_version`. Falls back to `"unknown"` when
//! the engine is built outside a git tree (published crate tarball,
//! shallow Docker layer without `.git`, etc.).

use std::path::Path;
use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=KIKAN_ENGINE_COMMIT={sha}");

    // Rebuild when HEAD or any ref moves so the baked SHA tracks the
    // working tree. Missing paths are fine — cargo treats them as absent.
    for rel in [
        "../../.git/HEAD",
        "../../.git/refs/heads",
        "../../.git/packed-refs",
    ] {
        if Path::new(rel).exists() {
            println!("cargo:rerun-if-changed={rel}");
        }
    }
}
