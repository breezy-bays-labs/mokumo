//! Mokumo's recovery-writer body. Wired into the engine via the closure
//! built at boot in `apps/mokumo-{server,desktop}` and registered on
//! [`kikan::BootConfig::with_recovery_writer`].
//!
//! Writes a Mokumo-branded HTML file containing the 6-digit PIN to a
//! known directory the operator can open locally. The filename derives
//! from a deterministic hash of the email so concurrent requests for
//! the same account overwrite the same file rather than multiplying.
//!
//! Kikan stays vocabulary-neutral; everything Mokumo-branded
//! (the `<title>` and `<h1>` text, the `mokumo-recovery-` filename
//! prefix) lives here.

use std::path::{Path, PathBuf};

use kikan::auth::recovery_artifact::{RecoveryArtifactLocation, RecoveryError};

/// Hash an email to a 16-hex-char filename component. Deterministic but
/// cryptographically toy — the recovery file lives in a directory the
/// operator already trusts; the hash just keeps the user's email out of
/// the on-disk filename.
fn hash_email_for_recovery_file(email: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in email.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Resolve the on-disk path of the recovery file for `email` under
/// `recovery_dir`. Stable for the same `(recovery_dir, email)` pair so
/// concurrent issuance overwrites the same file.
pub fn recovery_file_path_for_email(recovery_dir: &Path, email: &str) -> PathBuf {
    recovery_dir.join(format!(
        "mokumo-recovery-{}.html",
        hash_email_for_recovery_file(email)
    ))
}

pub fn recovery_html(pin: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>Mokumo Password Reset</title></head>
<body style="font-family:sans-serif;text-align:center;padding:4rem">
<h1>Mokumo Password Reset</h1>
<p>Enter this PIN in the application to reset your password:</p>
<p style="font-size:3rem;letter-spacing:0.5rem;font-weight:bold">{pin}</p>
<p style="color:#888">This PIN expires in 15 minutes.</p>
</body>
</html>"#
    )
}

/// Synchronous file write for the recovery-writer closure.
///
/// Creates `recovery_dir` if missing and writes a Mokumo-branded HTML
/// page containing `pin` to a deterministic per-email path. Uses an
/// atomic write-then-rename so a crash mid-write cannot leave a partial
/// or corrupted file at the final path. Returns the file's location for
/// the response payload, or surfaces I/O failure as [`RecoveryError::Io`].
pub fn write_recovery_artifact(
    email: &str,
    pin: &str,
    recovery_dir: &Path,
) -> Result<RecoveryArtifactLocation, RecoveryError> {
    use rand::RngExt;

    std::fs::create_dir_all(recovery_dir)?;
    let path = recovery_file_path_for_email(recovery_dir, email);
    // Per-call unique tmp suffix so two concurrent issuances for the same
    // email cannot overwrite each other's staged content before the
    // rename. Without this, an interleaving like
    // `A.write → B.write → A.rename → B.rename` would let A's rename
    // observe B's PIN bytes.
    let nonce: u64 = rand::rng().random();
    let tmp_path = path.with_extension(format!("html.tmp.{nonce:016x}"));
    std::fs::write(&tmp_path, recovery_html(pin))?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(RecoveryArtifactLocation::File { path })
}

#[cfg(test)]
mod tests {
    use super::{recovery_file_path_for_email, write_recovery_artifact};
    use kikan::auth::recovery_artifact::RecoveryArtifactLocation;
    use std::path::Path;

    #[test]
    fn recovery_file_path_is_stable_for_same_email() {
        let first = recovery_file_path_for_email(Path::new("/tmp"), "admin@shop.local");
        let second = recovery_file_path_for_email(Path::new("/tmp"), "admin@shop.local");
        assert_eq!(first, second);
    }

    #[test]
    fn recovery_file_path_differs_between_users() {
        let first = recovery_file_path_for_email(Path::new("/tmp"), "admin@shop.local");
        let second = recovery_file_path_for_email(Path::new("/tmp"), "staff@shop.local");
        assert_ne!(first, second);
    }

    #[test]
    fn write_recovery_artifact_creates_file_at_expected_path() {
        let dir = tempfile::tempdir().unwrap();
        let location = write_recovery_artifact("admin@shop.local", "123456", dir.path()).unwrap();
        let RecoveryArtifactLocation::File { path } = location;
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("123456"));
        assert!(body.contains("Mokumo Password Reset"));
    }

    #[test]
    fn write_recovery_artifact_creates_missing_recovery_dir() {
        let parent = tempfile::tempdir().unwrap();
        let nested = parent.path().join("nested").join("recovery");
        write_recovery_artifact("admin@shop.local", "654321", &nested).unwrap();
        assert!(nested.is_dir());
    }
}
