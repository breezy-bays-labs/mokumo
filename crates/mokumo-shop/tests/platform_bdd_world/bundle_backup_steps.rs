use std::collections::HashMap;
use std::path::PathBuf;

use cucumber::{given, then, when};
use kikan::{
    BundleBackupError, BundleManifest, BundleRestoreError, DbInBundle, RestoreTarget,
    create_bundle, restore_bundle,
};

use super::PlatformBddWorld;

const GROUP_ID: &str = "g-test";

pub struct BundleBackupCtx {
    pub data_dir: tempfile::TempDir,
    pub snapshot_root: PathBuf,
    /// Logical name -> source DB path under the data dir.
    pub sources: HashMap<String, PathBuf>,
    /// Logical name -> destination DB path used for restore scenarios.
    pub destinations: HashMap<String, PathBuf>,
    /// Pre-restore byte content of each destination, captured immediately
    /// before `restore_bundle` is invoked. The R6 strict-atomic invariant
    /// is asserted by comparing post-call destination bytes against this.
    pub destination_pre_restore: HashMap<String, Vec<u8>>,
    pub backup_result: Option<Result<BundleManifest, BundleBackupError>>,
    pub restore_result: Option<Result<(), BundleRestoreError>>,
}

impl std::fmt::Debug for BundleBackupCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BundleBackupCtx")
            .field("data_dir", &self.data_dir.path())
            .field("snapshot_root", &self.snapshot_root)
            .field("sources", &self.sources)
            .field("destinations", &self.destinations)
            .field("backup_result", &self.backup_result)
            .field("restore_result", &self.restore_result)
            .finish()
    }
}

fn write_seed_db(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE seed (id INTEGER PRIMARY KEY, payload TEXT);")
        .unwrap();
    conn.execute("INSERT INTO seed (payload) VALUES ('a'), ('b'), ('c')", [])
        .unwrap();
}

fn fresh_ctx() -> BundleBackupCtx {
    let data_dir = tempfile::tempdir().unwrap();
    let snapshot_root = data_dir.path().join("snapshots");
    BundleBackupCtx {
        data_dir,
        snapshot_root,
        sources: HashMap::new(),
        destinations: HashMap::new(),
        destination_pre_restore: HashMap::new(),
        backup_result: None,
        restore_result: None,
    }
}

// ── Backup-side steps ────────────────────────────────────────────────────

#[given("a data directory containing meta sessions and one profile database")]
async fn given_data_dir_with_three_dbs(w: &mut PlatformBddWorld) {
    let mut ctx = fresh_ctx();
    let logical_to_relpath = [
        ("meta", "meta.db"),
        ("sessions", "sessions.db"),
        ("vertical-acme", "acme/vertical.db"),
    ];
    for (logical, rel) in logical_to_relpath {
        let path = ctx.data_dir.path().join(rel);
        write_seed_db(&path);
        ctx.sources.insert(logical.to_string(), path);
    }
    w.bundle_backup = Some(ctx);
}

#[when("a bundle is created for the data directory")]
async fn when_bundle_created(w: &mut PlatformBddWorld) {
    let ctx = w.bundle_backup.as_mut().unwrap();
    let dbs: Vec<DbInBundle<'_>> = ctx
        .sources
        .iter()
        .map(|(name, path)| DbInBundle {
            logical_name: name.as_str(),
            source: path.as_path(),
        })
        .collect();
    ctx.backup_result = Some(create_bundle(&ctx.snapshot_root, GROUP_ID, &dbs).await);
}

#[then(expr = "the bundle manifest lists logical names {string} {string} and {string}")]
async fn then_manifest_lists(w: &mut PlatformBddWorld, a: String, b: String, c: String) {
    let ctx = w.bundle_backup.as_ref().unwrap();
    let manifest = ctx
        .backup_result
        .as_ref()
        .expect("bundle was created")
        .as_ref()
        .expect("bundle creation succeeded");
    let mut names: Vec<&str> = manifest
        .entries
        .iter()
        .map(|e| e.logical_name.as_str())
        .collect();
    names.sort();
    let mut expected = [a.as_str(), b.as_str(), c.as_str()];
    expected.sort();
    assert_eq!(names, expected.to_vec());
}

#[then("each logical name has a snapshot file on disk")]
async fn then_snapshots_on_disk(w: &mut PlatformBddWorld) {
    let ctx = w.bundle_backup.as_ref().unwrap();
    let manifest = ctx
        .backup_result
        .as_ref()
        .expect("bundle was created")
        .as_ref()
        .expect("bundle creation succeeded");
    let group_dir = ctx.snapshot_root.join(&manifest.group_id);
    for entry in &manifest.entries {
        let path = group_dir.join(&entry.snapshot_filename);
        assert!(
            path.exists(),
            "snapshot file for `{}` missing at {}",
            entry.logical_name,
            path.display(),
        );
        assert!(
            path.metadata().unwrap().len() > 0,
            "snapshot file for `{}` is empty",
            entry.logical_name,
        );
    }
}

#[then("every snapshot passes a SQLite integrity check")]
async fn then_snapshots_integrity_check(w: &mut PlatformBddWorld) {
    let ctx = w.bundle_backup.as_ref().unwrap();
    let manifest = ctx
        .backup_result
        .as_ref()
        .expect("bundle was created")
        .as_ref()
        .expect("bundle creation succeeded");
    let group_dir = ctx.snapshot_root.join(&manifest.group_id);
    for entry in &manifest.entries {
        let path = group_dir.join(&entry.snapshot_filename);
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap_or_else(|e| panic!("failed to open snapshot {}: {e}", path.display()));
        let row: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            row,
            "ok",
            "integrity_check failed for `{}` at {}",
            entry.logical_name,
            path.display(),
        );
    }
}

// ── Restore-side steps ───────────────────────────────────────────────────

#[given("a snapshots directory with a bundle group containing 3 healthy database snapshots")]
async fn given_healthy_bundle_group(w: &mut PlatformBddWorld) {
    let mut ctx = fresh_ctx();
    let logical_to_relpath = [
        ("meta", "meta.db"),
        ("sessions", "sessions.db"),
        ("vertical-acme", "acme/vertical.db"),
    ];
    let mut dbs_owned: Vec<(String, PathBuf)> = Vec::new();
    for (logical, rel) in logical_to_relpath {
        let path = ctx.data_dir.path().join(rel);
        write_seed_db(&path);
        ctx.sources.insert(logical.to_string(), path.clone());
        dbs_owned.push((logical.to_string(), path));
    }
    let dbs: Vec<DbInBundle<'_>> = dbs_owned
        .iter()
        .map(|(name, path)| DbInBundle {
            logical_name: name.as_str(),
            source: path.as_path(),
        })
        .collect();
    create_bundle(&ctx.snapshot_root, GROUP_ID, &dbs)
        .await
        .expect("bundle creation succeeded");
    w.bundle_backup = Some(ctx);
}

#[given(expr = "the snapshot for {string} is corrupted on disk")]
async fn given_snapshot_corrupted(w: &mut PlatformBddWorld, logical_name: String) {
    let ctx = w.bundle_backup.as_ref().unwrap();
    let snap = ctx
        .snapshot_root
        .join(GROUP_ID)
        .join(format!("{logical_name}.db"));
    assert!(
        snap.exists(),
        "expected snapshot at {} before corrupting",
        snap.display()
    );
    std::fs::write(&snap, b"not a sqlite database").unwrap();
}

#[given("the bundle manifest is removed")]
async fn given_manifest_removed(w: &mut PlatformBddWorld) {
    let ctx = w.bundle_backup.as_ref().unwrap();
    let manifest = ctx.snapshot_root.join(GROUP_ID).join("manifest.json");
    std::fs::remove_file(&manifest).unwrap();
}

#[given("destination database files exist with known content")]
async fn given_destinations_exist(w: &mut PlatformBddWorld) {
    let ctx = w.bundle_backup.as_mut().unwrap();
    let dest_root = ctx.data_dir.path().join("restored");
    for (logical, _src) in ctx.sources.clone() {
        let dest = dest_root.join(format!("{logical}.db"));
        write_seed_db(&dest);
        // Stamp each destination with a unique payload so the byte-equality
        // check after restore would visibly fail if rename(2) ran.
        {
            let conn = rusqlite::Connection::open(&dest).unwrap();
            conn.execute(
                "INSERT INTO seed (payload) VALUES (?1)",
                [&format!("pre-restore-{logical}")],
            )
            .unwrap();
        }
        let bytes = std::fs::read(&dest).unwrap();
        ctx.destinations.insert(logical.clone(), dest);
        ctx.destination_pre_restore.insert(logical, bytes);
    }
}

#[when("the bundle group is restored to the data directory")]
async fn when_bundle_restored(w: &mut PlatformBddWorld) {
    let ctx = w.bundle_backup.as_mut().unwrap();
    let targets: Vec<RestoreTarget> = ctx
        .destinations
        .iter()
        .map(|(name, dest)| RestoreTarget {
            logical_name: name.clone(),
            dest: dest.clone(),
        })
        .collect();
    ctx.restore_result = Some(restore_bundle(&ctx.snapshot_root, GROUP_ID, &targets).await);
}

#[then(expr = "restore fails with BundleRestoreError PartialCorruption naming {string}")]
async fn then_restore_partial_corruption(w: &mut PlatformBddWorld, logical_name: String) {
    let ctx = w.bundle_backup.as_ref().unwrap();
    let err = ctx
        .restore_result
        .as_ref()
        .expect("restore was attempted")
        .as_ref()
        .expect_err("restore must fail");
    match err {
        BundleRestoreError::PartialCorruption { failed_file, .. } => assert!(
            failed_file
                .to_string_lossy()
                .ends_with(&format!("{logical_name}.db")),
            "PartialCorruption failed_file `{}` does not end with `{logical_name}.db`",
            failed_file.display(),
        ),
        other => panic!("expected PartialCorruption, got {other:?}"),
    }
}

#[then("restore fails with BundleRestoreError ManifestVerificationFailed")]
async fn then_restore_manifest_verification_failed(w: &mut PlatformBddWorld) {
    let ctx = w.bundle_backup.as_ref().unwrap();
    let err = ctx
        .restore_result
        .as_ref()
        .expect("restore was attempted")
        .as_ref()
        .expect_err("restore must fail");
    assert!(
        matches!(err, BundleRestoreError::ManifestVerificationFailed { .. }),
        "expected ManifestVerificationFailed, got {err:?}"
    );
}

#[then("every destination database file is byte-identical to its pre-restore content")]
async fn then_destinations_unchanged(w: &mut PlatformBddWorld) {
    let ctx = w.bundle_backup.as_ref().unwrap();
    for (logical, dest) in &ctx.destinations {
        let pre = ctx
            .destination_pre_restore
            .get(logical)
            .expect("pre-restore content captured");
        let post = std::fs::read(dest).unwrap_or_else(|e| {
            panic!(
                "destination for `{logical}` at {} could not be read after refusal: {e}",
                dest.display()
            )
        });
        assert_eq!(
            &post,
            pre,
            "destination for `{logical}` at {} was mutated by a refused restore",
            dest.display(),
        );
    }
}
