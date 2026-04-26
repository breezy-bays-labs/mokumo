//! Multi-database bundle backup + strict-atomic restore.
//!
//! A *bundle* is the operator-facing unit of backup: meta.db,
//! sessions.db, and every per-profile vertical DB captured into a
//! single point-in-time group. Each database in the bundle is
//! snapshotted via SQLite `VACUUM INTO`, which checkpoints the WAL
//! into a self-contained file rather than relying on a torn-image
//! file copy of an open WAL-mode database.
//!
//! The primitive is intentionally vocabulary-neutral: callers hand it
//! a list of `[DbInBundle]` entries (logical name + on-disk source
//! path) so kikan never needs to know about meta.profiles, vertical
//! `db_filename` values, or per-profile slug naming.
//!
//! # Strict-atomic restore (R6)
//!
//! Restore refuses to mutate any destination file unless every
//! snapshot named in the manifest passes `PRAGMA integrity_check`.
//! On any verification failure the destination tree is left untouched
//! and a [`BundleRestoreError`] names the offending logical entry.
//! There is no best-effort partial-restore path; the operator-facing
//! contract is "we refused, you know exactly where you stand" rather
//! than a half-restored install.
//!
//! See `adr-kikan-upgrade-migration-strategy.md` §"Multi-database
//! operation-level atomicity via snapshot-and-restore" for the
//! load-bearing precedent (manifest + `VACUUM INTO` + atomic rename).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Bundle manifest schema version. Bumping this signals a breaking
/// change in `manifest.json`; restore refuses any manifest whose
/// version does not equal this constant.
pub const BUNDLE_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// One database to include in a bundle.
///
/// `logical_name` is the caller-chosen identifier under which the
/// snapshot is stored in the bundle and reported back through the
/// manifest. It must be a non-empty filesystem-safe string (no path
/// separators, no `..`); the primitive validates this at bundle time.
#[derive(Debug, Clone)]
pub struct DbInBundle<'a> {
    pub logical_name: &'a str,
    pub source: &'a Path,
}

/// Restore target — pairs a manifest's `logical_name` with the
/// destination path the snapshot should land at on success.
#[derive(Debug, Clone)]
pub struct RestoreTarget {
    pub logical_name: String,
    pub dest: PathBuf,
}

/// On-disk manifest entry for one database snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundleManifestEntry {
    pub logical_name: String,
    pub snapshot_filename: String,
    pub bytes: u64,
}

/// On-disk bundle manifest.
///
/// `entries` is sorted by `logical_name` for deterministic output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub group_id: String,
    pub created_at: DateTime<Utc>,
    pub entries: Vec<BundleManifestEntry>,
}

impl BundleManifest {
    fn entries_by_name(&self) -> BTreeMap<&str, &BundleManifestEntry> {
        self.entries
            .iter()
            .map(|e| (e.logical_name.as_str(), e))
            .collect()
    }
}

/// Errors produced by [`create_bundle`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BundleBackupError {
    #[error("bundle must contain at least one database")]
    EmptyBundle,

    #[error("logical name `{name}` is invalid: {reason}")]
    InvalidLogicalName { name: String, reason: &'static str },

    #[error("duplicate logical name `{name}` in bundle inputs")]
    DuplicateLogicalName { name: String },

    #[error("source database for `{logical_name}` does not exist: {}", path.display())]
    SourceMissing { logical_name: String, path: PathBuf },

    #[error("snapshot of `{logical_name}` failed: {source}")]
    Snapshot {
        logical_name: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    #[error("io error in snapshot directory {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to serialize bundle manifest: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Errors produced by [`restore_bundle`].
///
/// `PartialCorruption` and `ManifestVerificationFailed` together
/// encode R6's strict-atomic refusal contract: returned BEFORE any
/// destination file is touched.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BundleRestoreError {
    #[error("bundle manifest verification failed: {reason}")]
    ManifestVerificationFailed { reason: String },

    #[error("snapshot for `{}` failed integrity check ({reason}); no destination files were modified", failed_file.display())]
    PartialCorruption {
        failed_file: PathBuf,
        reason: String,
    },

    #[error("restore target `{logical_name}` has no matching manifest entry")]
    UnknownTarget { logical_name: String },

    #[error("manifest entry `{logical_name}` has no matching restore target")]
    UnmatchedManifestEntry { logical_name: String },

    #[error(
        "snapshot file for `{logical_name}` is missing on disk: {}",
        path.display()
    )]
    SnapshotMissing { logical_name: String, path: PathBuf },

    #[error("filesystem error during restore at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Create a bundle group at `<snapshot_root>/<group_id>/`.
///
/// Writes `manifest.json` and one `<snapshot_filename>` per entry.
/// Each snapshot is produced via `VACUUM INTO` against the source DB,
/// so WAL-mode sources are checkpointed into the snapshot rather than
/// captured as a torn image.
///
/// Validation: `dbs` must be non-empty, every `logical_name` must be
/// filesystem-safe (`[A-Za-z0-9._-]+` and no `..`), names must be
/// unique. Source files must exist.
///
/// The function is async because `VACUUM INTO` runs in
/// `spawn_blocking` (rusqlite is sync); the rest is straight
/// filesystem work.
pub async fn create_bundle(
    snapshot_root: &Path,
    group_id: &str,
    dbs: &[DbInBundle<'_>],
) -> Result<BundleManifest, BundleBackupError> {
    validate_inputs(dbs)?;

    let group_dir = snapshot_root.join(group_id);
    create_group_dir(&group_dir)?;

    let mut entries = Vec::with_capacity(dbs.len());
    for db in dbs {
        if !db.source.exists() {
            return Err(BundleBackupError::SourceMissing {
                logical_name: db.logical_name.to_string(),
                path: db.source.to_path_buf(),
            });
        }
        let snapshot_filename = format!("{}.db", db.logical_name);
        let snapshot_path = group_dir.join(&snapshot_filename);

        vacuum_into_snapshot(db.logical_name, db.source, &snapshot_path).await?;

        let bytes = std::fs::metadata(&snapshot_path)
            .map_err(|source| BundleBackupError::Io {
                path: snapshot_path.clone(),
                source,
            })?
            .len();

        entries.push(BundleManifestEntry {
            logical_name: db.logical_name.to_string(),
            snapshot_filename,
            bytes,
        });
    }
    entries.sort_by(|a, b| a.logical_name.cmp(&b.logical_name));

    let manifest = BundleManifest {
        schema_version: BUNDLE_MANIFEST_SCHEMA_VERSION,
        group_id: group_id.to_string(),
        created_at: Utc::now(),
        entries,
    };

    write_manifest(&group_dir, &manifest)?;

    Ok(manifest)
}

/// Restore a bundle group with strict-atomic semantics (R6).
///
/// Steps:
/// 1. Read + verify `manifest.json` (`schema_version` matches; every
///    entry exists on disk; targets are bijective with manifest
///    entries).
/// 2. Run `PRAGMA integrity_check` on every snapshot file.
/// 3. Only after every check above passes, atomically `rename(2)`
///    each snapshot into its destination.
///
/// On any failure during steps 1 or 2 no destination file is
/// touched. A failure during step 3 (rare — same-filesystem rename
/// on POSIX) surfaces as [`BundleRestoreError::Io`]; per the ADR,
/// cross-file aggregate atomicity is not provided, only "refuse
/// before mutating" is guaranteed.
pub async fn restore_bundle(
    snapshot_root: &Path,
    group_id: &str,
    targets: &[RestoreTarget],
) -> Result<(), BundleRestoreError> {
    let group_dir = snapshot_root.join(group_id);
    let manifest = read_manifest(&group_dir)?;
    let pairs = pair_targets_with_entries(&group_dir, &manifest, targets)?;

    for (snapshot_path, _) in &pairs {
        verify_snapshot_integrity(snapshot_path).await?;
    }
    precreate_destination_dirs(&pairs)?;

    atomic_rename_all(&pairs)?;
    Ok(())
}

fn validate_inputs(dbs: &[DbInBundle<'_>]) -> Result<(), BundleBackupError> {
    if dbs.is_empty() {
        return Err(BundleBackupError::EmptyBundle);
    }
    let mut seen = std::collections::HashSet::with_capacity(dbs.len());
    for db in dbs {
        validate_logical_name(db.logical_name)?;
        if !seen.insert(db.logical_name) {
            return Err(BundleBackupError::DuplicateLogicalName {
                name: db.logical_name.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_logical_name(name: &str) -> Result<(), BundleBackupError> {
    if name.is_empty() {
        return Err(BundleBackupError::InvalidLogicalName {
            name: name.to_string(),
            reason: "empty",
        });
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(BundleBackupError::InvalidLogicalName {
            name: name.to_string(),
            reason: "must not contain path separators or be `.`/`..`",
        });
    }
    let ok = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
    if !ok {
        return Err(BundleBackupError::InvalidLogicalName {
            name: name.to_string(),
            reason: "must be ASCII [A-Za-z0-9._-]",
        });
    }
    Ok(())
}

fn create_group_dir(group_dir: &Path) -> Result<(), BundleBackupError> {
    std::fs::create_dir_all(group_dir).map_err(|source| BundleBackupError::Io {
        path: group_dir.to_path_buf(),
        source,
    })
}

async fn vacuum_into_snapshot(
    logical_name: &str,
    source: &Path,
    snapshot_path: &Path,
) -> Result<(), BundleBackupError> {
    // VACUUM INTO requires the destination to NOT exist. We first vacuum
    // into `<snapshot>.tmp` and only rename(2) it onto the final path on
    // success. If the process is interrupted mid-vacuum, the previous
    // (good) snapshot at `snapshot_path` is preserved and only the
    // `.tmp` is left behind for cleanup on retry.
    let tmp_path = tmp_sibling(snapshot_path);
    if tmp_path.exists() {
        std::fs::remove_file(&tmp_path).map_err(|source| BundleBackupError::Io {
            path: tmp_path.clone(),
            source,
        })?;
    }
    // Reject non-UTF-8 destinations rather than silently lossy-coerce —
    // SQLite's `VACUUM INTO` parses the path string and a lossy round
    // trip can land bytes outside the originally intended file.
    let tmp_str = tmp_path
        .to_str()
        .ok_or_else(|| BundleBackupError::InvalidLogicalName {
            name: logical_name.to_string(),
            reason: "snapshot path is not valid UTF-8",
        })?
        .to_string();
    let source = source.to_path_buf();
    let logical = logical_name.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), rusqlite::Error> {
        let conn = rusqlite::Connection::open_with_flags(
            &source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // VACUUM INTO checkpoints WAL pages into a self-contained file
        // without holding the source's write lock for the full copy.
        // Bind the destination as a parameter — rusqlite escapes it for
        // SQLite's quoting rules.
        conn.execute("VACUUM INTO ?1", [tmp_str.as_str()])?;
        Ok(())
    })
    .await
    .map_err(|join_err| BundleBackupError::Snapshot {
        logical_name: logical.clone(),
        source: Box::new(join_err),
    })?
    .map_err(|sql_err| BundleBackupError::Snapshot {
        logical_name: logical.clone(),
        source: Box::new(sql_err),
    })?;
    // Promote the temp into place. `rename(2)` is atomic on the same
    // filesystem; the temp sits next to the destination so this is the
    // common case.
    std::fs::rename(&tmp_path, snapshot_path).map_err(|source| BundleBackupError::Io {
        path: snapshot_path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Hard cap on `manifest.json` size during read. Manifests are tens
/// of bytes per entry; a real install with hundreds of profiles tops
/// out under a megabyte. Refusing anything larger keeps a malicious
/// or corrupted file from forcing an unbounded allocation.
const MANIFEST_MAX_BYTES: u64 = 16 * 1024 * 1024;

fn write_manifest(group_dir: &Path, manifest: &BundleManifest) -> Result<(), BundleBackupError> {
    // Atomic write: serialize → write to `manifest.json.tmp` → rename
    // into place. If we crash mid-write, the operator sees either the
    // previous valid manifest or no manifest at all (the latter trips
    // `ManifestVerificationFailed` on restore), never a half-written one
    // that parses partially.
    let manifest_path = group_dir.join("manifest.json");
    let tmp_path = tmp_sibling(&manifest_path);
    let json = serde_json::to_vec_pretty(manifest)?;
    std::fs::write(&tmp_path, json).map_err(|source| BundleBackupError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    std::fs::rename(&tmp_path, &manifest_path).map_err(|source| BundleBackupError::Io {
        path: manifest_path,
        source,
    })?;
    Ok(())
}

fn read_manifest(group_dir: &Path) -> Result<BundleManifest, BundleRestoreError> {
    let manifest_path = group_dir.join("manifest.json");
    let metadata = std::fs::metadata(&manifest_path).map_err(|e| {
        BundleRestoreError::ManifestVerificationFailed {
            reason: format!("could not stat {}: {e}", manifest_path.display()),
        }
    })?;
    if metadata.len() > MANIFEST_MAX_BYTES {
        return Err(BundleRestoreError::ManifestVerificationFailed {
            reason: format!(
                "manifest at {} is {} bytes, exceeds {}-byte cap",
                manifest_path.display(),
                metadata.len(),
                MANIFEST_MAX_BYTES
            ),
        });
    }
    let bytes = std::fs::read(&manifest_path).map_err(|e| {
        BundleRestoreError::ManifestVerificationFailed {
            reason: format!("could not read {}: {e}", manifest_path.display()),
        }
    })?;
    let manifest: BundleManifest = serde_json::from_slice(&bytes).map_err(|e| {
        BundleRestoreError::ManifestVerificationFailed {
            reason: format!("malformed manifest at {}: {e}", manifest_path.display()),
        }
    })?;
    if manifest.schema_version != BUNDLE_MANIFEST_SCHEMA_VERSION {
        return Err(BundleRestoreError::ManifestVerificationFailed {
            reason: format!(
                "manifest schema_version {} does not match supported {}",
                manifest.schema_version, BUNDLE_MANIFEST_SCHEMA_VERSION,
            ),
        });
    }
    Ok(manifest)
}

fn pair_targets_with_entries(
    group_dir: &Path,
    manifest: &BundleManifest,
    targets: &[RestoreTarget],
) -> Result<Vec<(PathBuf, PathBuf)>, BundleRestoreError> {
    let by_name = manifest.entries_by_name();
    let mut pairs = Vec::with_capacity(targets.len());
    let mut matched = std::collections::HashSet::with_capacity(targets.len());
    for target in targets {
        let entry = by_name.get(target.logical_name.as_str()).ok_or_else(|| {
            BundleRestoreError::UnknownTarget {
                logical_name: target.logical_name.clone(),
            }
        })?;
        let snapshot_path = group_dir.join(&entry.snapshot_filename);
        if !snapshot_path.exists() {
            return Err(BundleRestoreError::SnapshotMissing {
                logical_name: target.logical_name.clone(),
                path: snapshot_path,
            });
        }
        matched.insert(entry.logical_name.as_str());
        pairs.push((snapshot_path, target.dest.clone()));
    }
    if let Some(entry) = manifest
        .entries
        .iter()
        .find(|e| !matched.contains(e.logical_name.as_str()))
    {
        return Err(BundleRestoreError::UnmatchedManifestEntry {
            logical_name: entry.logical_name.clone(),
        });
    }
    Ok(pairs)
}

async fn verify_snapshot_integrity(snapshot_path: &Path) -> Result<(), BundleRestoreError> {
    let owned = snapshot_path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || -> Result<String, rusqlite::Error> {
        let conn = rusqlite::Connection::open_with_flags(
            &owned,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let row: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(row)
    })
    .await;
    // Preserve the underlying failure mode in the structured error so
    // operators don't have to grep logs to know whether the snapshot
    // was unreadable, malformed, or merely failed page-level checks.
    let reason = match result {
        Ok(Ok(ref row)) if row == "ok" => return Ok(()),
        Ok(Ok(other)) => format!("integrity_check returned `{other}`"),
        Ok(Err(sql_err)) => format!("could not open or query snapshot: {sql_err}"),
        Err(join_err) => format!("integrity-check task failed: {join_err}"),
    };
    Err(BundleRestoreError::PartialCorruption {
        failed_file: snapshot_path.to_path_buf(),
        reason,
    })
}

/// Pre-create every destination's parent directory. Doing this in
/// the verify pass (before any rename) means a directory permission
/// or disk-full error surfaces while the on-disk state is still
/// strictly the pre-restore tree.
fn precreate_destination_dirs(pairs: &[(PathBuf, PathBuf)]) -> Result<(), BundleRestoreError> {
    for (_, dest) in pairs {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|source| BundleRestoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    Ok(())
}

fn atomic_rename_all(pairs: &[(PathBuf, PathBuf)]) -> Result<(), BundleRestoreError> {
    for (src, dest) in pairs {
        move_or_copy(src, dest)?;
    }
    Ok(())
}

/// Move `src` to `dest`. Tries `rename(2)` first (atomic on the same
/// filesystem); on `EXDEV` (cross-filesystem) falls back to copy +
/// remove. The fallback is not byte-atomic from the destination's
/// perspective — callers that need that should keep snapshots and
/// destinations on the same mount.
fn move_or_copy(src: &Path, dest: &Path) -> Result<(), BundleRestoreError> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            std::fs::copy(src, dest).map_err(|source| BundleRestoreError::Io {
                path: dest.to_path_buf(),
                source,
            })?;
            // Best-effort cleanup; failure here doesn't break the
            // operator-visible contract — the destination is in place.
            let _ = std::fs::remove_file(src);
            Ok(())
        }
        Err(source) => Err(BundleRestoreError::Io {
            path: dest.to_path_buf(),
            source,
        }),
    }
}

fn is_cross_device(e: &std::io::Error) -> bool {
    if matches!(e.kind(), std::io::ErrorKind::CrossesDevices) {
        return true;
    }
    // Defensive fallback for older platform mappings that may still
    // surface EXDEV as `ErrorKind::Other`. EXDEV is 18 on Linux and
    // most BSDs, 17 on Solaris-derived Unixes; on Windows the
    // equivalent (`ERROR_NOT_SAME_DEVICE`) is 17.
    matches!(e.raw_os_error(), Some(17) | Some(18))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_seed_db(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE seed (id INTEGER PRIMARY KEY, payload TEXT);")
            .unwrap();
        conn.execute("INSERT INTO seed (payload) VALUES ('a'), ('b'), ('c')", [])
            .unwrap();
    }

    fn read_bytes(p: &Path) -> Vec<u8> {
        std::fs::read(p).unwrap()
    }

    #[tokio::test]
    async fn create_bundle_rejects_empty_input() {
        let dir = tempfile::tempdir().unwrap();
        let err = create_bundle(dir.path(), "g1", &[]).await.unwrap_err();
        assert!(matches!(err, BundleBackupError::EmptyBundle));
    }

    #[tokio::test]
    async fn create_bundle_rejects_invalid_logical_name() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.db");
        write_seed_db(&src);
        let err = create_bundle(
            dir.path(),
            "g1",
            &[DbInBundle {
                logical_name: "bad/name",
                source: &src,
            }],
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, BundleBackupError::InvalidLogicalName { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn create_bundle_rejects_duplicate_logical_names() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.db");
        write_seed_db(&src);
        let err = create_bundle(
            dir.path(),
            "g1",
            &[
                DbInBundle {
                    logical_name: "alpha",
                    source: &src,
                },
                DbInBundle {
                    logical_name: "alpha",
                    source: &src,
                },
            ],
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            BundleBackupError::DuplicateLogicalName { .. }
        ));
    }

    #[tokio::test]
    async fn create_bundle_writes_manifest_and_snapshots() {
        let work = tempfile::tempdir().unwrap();
        let snaps = work.path().join("snaps");
        let src_meta = work.path().join("meta.db");
        let src_profile = work.path().join("acme/vertical.db");
        write_seed_db(&src_meta);
        write_seed_db(&src_profile);

        let manifest = create_bundle(
            &snaps,
            "g1",
            &[
                DbInBundle {
                    logical_name: "meta",
                    source: &src_meta,
                },
                DbInBundle {
                    logical_name: "vertical-acme",
                    source: &src_profile,
                },
            ],
        )
        .await
        .unwrap();

        assert_eq!(manifest.schema_version, BUNDLE_MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.group_id, "g1");
        let names: Vec<&str> = manifest
            .entries
            .iter()
            .map(|e| e.logical_name.as_str())
            .collect();
        assert_eq!(names, vec!["meta", "vertical-acme"]);

        let group = snaps.join("g1");
        assert!(group.join("manifest.json").exists());
        assert!(group.join("meta.db").exists());
        assert!(group.join("vertical-acme.db").exists());
    }

    #[tokio::test]
    async fn restore_bundle_round_trips_payload() {
        let work = tempfile::tempdir().unwrap();
        let snaps = work.path().join("snaps");
        let src = work.path().join("src/data.db");
        write_seed_db(&src);

        create_bundle(
            &snaps,
            "g1",
            &[DbInBundle {
                logical_name: "data",
                source: &src,
            }],
        )
        .await
        .unwrap();

        let dest = work.path().join("restored/data.db");
        restore_bundle(
            &snaps,
            "g1",
            &[RestoreTarget {
                logical_name: "data".into(),
                dest: dest.clone(),
            }],
        )
        .await
        .unwrap();

        let conn = rusqlite::Connection::open(&dest).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM seed", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn restore_refuses_when_manifest_missing() {
        let work = tempfile::tempdir().unwrap();
        let snaps = work.path().join("snaps");
        let src = work.path().join("src.db");
        write_seed_db(&src);
        create_bundle(
            &snaps,
            "g1",
            &[DbInBundle {
                logical_name: "data",
                source: &src,
            }],
        )
        .await
        .unwrap();
        std::fs::remove_file(snaps.join("g1/manifest.json")).unwrap();

        let dest = work.path().join("dest.db");
        write_seed_db(&dest);
        let pre = read_bytes(&dest);

        let err = restore_bundle(
            &snaps,
            "g1",
            &[RestoreTarget {
                logical_name: "data".into(),
                dest: dest.clone(),
            }],
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            BundleRestoreError::ManifestVerificationFailed { .. }
        ));
        assert_eq!(read_bytes(&dest), pre, "destination must be untouched");
    }

    #[tokio::test]
    async fn restore_refuses_on_partial_corruption_with_disk_unchanged() {
        let work = tempfile::tempdir().unwrap();
        let snaps = work.path().join("snaps");
        let src_a = work.path().join("a.db");
        let src_b = work.path().join("b.db");
        write_seed_db(&src_a);
        write_seed_db(&src_b);
        create_bundle(
            &snaps,
            "g1",
            &[
                DbInBundle {
                    logical_name: "alpha",
                    source: &src_a,
                },
                DbInBundle {
                    logical_name: "bravo",
                    source: &src_b,
                },
            ],
        )
        .await
        .unwrap();

        // Corrupt the bravo snapshot — overwrite the file with bytes
        // that are not a valid SQLite database. PRAGMA integrity_check
        // cannot run because Connection::open rejects the file; the
        // primitive's verify step treats either open failure or
        // non-"ok" integrity as PartialCorruption.
        let bravo_snap = snaps.join("g1/bravo.db");
        std::fs::write(&bravo_snap, b"not a sqlite database").unwrap();

        let dest_a = work.path().join("restored/a.db");
        let dest_b = work.path().join("restored/b.db");
        write_seed_db(&dest_a);
        write_seed_db(&dest_b);
        let pre_a = read_bytes(&dest_a);
        let pre_b = read_bytes(&dest_b);

        let err = restore_bundle(
            &snaps,
            "g1",
            &[
                RestoreTarget {
                    logical_name: "alpha".into(),
                    dest: dest_a.clone(),
                },
                RestoreTarget {
                    logical_name: "bravo".into(),
                    dest: dest_b.clone(),
                },
            ],
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, BundleRestoreError::PartialCorruption { ref failed_file, .. } if failed_file.ends_with("bravo.db")),
            "got {err:?}"
        );
        assert_eq!(read_bytes(&dest_a), pre_a, "alpha destination touched");
        assert_eq!(read_bytes(&dest_b), pre_b, "bravo destination touched");
    }

    #[tokio::test]
    async fn restore_rejects_target_unknown_to_manifest() {
        let work = tempfile::tempdir().unwrap();
        let snaps = work.path().join("snaps");
        let src = work.path().join("src.db");
        write_seed_db(&src);
        create_bundle(
            &snaps,
            "g1",
            &[DbInBundle {
                logical_name: "data",
                source: &src,
            }],
        )
        .await
        .unwrap();

        let dest = work.path().join("dest.db");
        let err = restore_bundle(
            &snaps,
            "g1",
            &[RestoreTarget {
                logical_name: "missing".into(),
                dest,
            }],
        )
        .await
        .unwrap_err();
        assert!(matches!(err, BundleRestoreError::UnknownTarget { .. }));
    }

    #[tokio::test]
    async fn restore_rejects_manifest_entry_without_target() {
        let work = tempfile::tempdir().unwrap();
        let snaps = work.path().join("snaps");
        let src_a = work.path().join("a.db");
        let src_b = work.path().join("b.db");
        write_seed_db(&src_a);
        write_seed_db(&src_b);
        create_bundle(
            &snaps,
            "g1",
            &[
                DbInBundle {
                    logical_name: "alpha",
                    source: &src_a,
                },
                DbInBundle {
                    logical_name: "bravo",
                    source: &src_b,
                },
            ],
        )
        .await
        .unwrap();

        let dest = work.path().join("dest.db");
        let err = restore_bundle(
            &snaps,
            "g1",
            &[RestoreTarget {
                logical_name: "alpha".into(),
                dest,
            }],
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            BundleRestoreError::UnmatchedManifestEntry { .. }
        ));
    }

    #[tokio::test]
    async fn partial_corruption_carries_reason_string() {
        let work = tempfile::tempdir().unwrap();
        let snaps = work.path().join("snaps");
        let src = work.path().join("src.db");
        write_seed_db(&src);
        create_bundle(
            &snaps,
            "g1",
            &[DbInBundle {
                logical_name: "data",
                source: &src,
            }],
        )
        .await
        .unwrap();
        std::fs::write(snaps.join("g1/data.db"), b"not a sqlite database").unwrap();

        let dest = work.path().join("dest.db");
        let err = restore_bundle(
            &snaps,
            "g1",
            &[RestoreTarget {
                logical_name: "data".into(),
                dest,
            }],
        )
        .await
        .unwrap_err();
        match err {
            BundleRestoreError::PartialCorruption { reason, .. } => assert!(
                !reason.is_empty(),
                "PartialCorruption.reason must describe the failure",
            ),
            other => panic!("expected PartialCorruption, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_manifest_refuses_oversize_file() {
        let work = tempfile::tempdir().unwrap();
        let group_dir = work.path().join("g1");
        std::fs::create_dir_all(&group_dir).unwrap();
        // Use a sparse file via set_len to avoid writing actual MBs.
        let manifest_path = group_dir.join("manifest.json");
        let f = std::fs::File::create(&manifest_path).unwrap();
        f.set_len(MANIFEST_MAX_BYTES + 1).unwrap();

        let err = read_manifest(&group_dir).unwrap_err();
        match err {
            BundleRestoreError::ManifestVerificationFailed { reason } => {
                assert!(
                    reason.contains("exceeds") && reason.contains("cap"),
                    "expected size-cap reason, got `{reason}`",
                );
            }
            other => panic!("expected ManifestVerificationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_bundle_does_not_leave_tmp_on_success() {
        let work = tempfile::tempdir().unwrap();
        let snaps = work.path().join("snaps");
        let src = work.path().join("src.db");
        write_seed_db(&src);
        create_bundle(
            &snaps,
            "g1",
            &[DbInBundle {
                logical_name: "data",
                source: &src,
            }],
        )
        .await
        .unwrap();
        let group = snaps.join("g1");
        assert!(group.join("data.db").exists());
        assert!(
            !group.join("data.db.tmp").exists(),
            "snapshot temp must be promoted on success",
        );
        assert!(
            !group.join("manifest.json.tmp").exists(),
            "manifest temp must be promoted on success",
        );
    }

    #[test]
    fn is_cross_device_recognizes_known_codes() {
        for code in [17, 18] {
            let e = std::io::Error::from_raw_os_error(code);
            assert!(is_cross_device(&e), "code {code} should classify as EXDEV");
        }
        // ENOENT is not a cross-device condition.
        let other = std::io::Error::from_raw_os_error(2);
        assert!(!is_cross_device(&other));
    }
}
