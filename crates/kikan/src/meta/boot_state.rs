//! Boot-state detection — figures out what kind of data directory we're
//! looking at so the engine can decide whether to do a fresh install,
//! a normal boot, a legacy upgrade, or refuse with a defensive error.
//!
//! Each variant carries the data its corresponding handler needs (e.g.
//! `LegacyCompleted` carries the `shop_name` read from the legacy
//! per-profile DB so the upgrade handler doesn't have to re-open it).
//!
//! Detection is read-only: the meta pool is queried for `meta.profiles`,
//! the legacy per-profile DB is opened with `SQLITE_OPEN_READ_ONLY` so
//! probing never mutates state. See `adr-kikan-upgrade-migration-strategy.md`
//! and the M00 shape doc §Seam 1.

use std::path::{Path, PathBuf};

use rusqlite::OpenFlags;
use sea_orm::{DatabaseConnection, EntityTrait, PaginatorTrait};
use thiserror::Error;

use crate::meta::entity::profile;

/// Why a `production/` folder was classified as abandoned mid-setup.
///
/// The boot dispatcher logs this so the operator can tell the difference
/// between "wizard never finished writing the per-profile DB file" and
/// "wizard wrote the file but never created an admin user".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbandonReason {
    /// `<production>/<vertical>.db` is missing entirely.
    NoVerticalDbFile,
    /// `<production>/<vertical>.db` exists but the `users` table has zero
    /// rows with role_id = 1 (Admin) AND is_active = 1 AND deleted_at IS NULL.
    NoAdminUser,
}

/// Result of inspecting `<data_dir>` to decide the boot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootState {
    /// No `meta.profiles` rows and no `production/` folder. New install —
    /// run the setup wizard.
    FreshInstall,

    /// `meta.db` has at least one row in `meta.profiles`. Normal boot path —
    /// open per-profile pools and serve traffic. `profile_count` lets the
    /// dispatcher log a summary without re-querying.
    PostUpgradeOrSetup { profile_count: usize },

    /// `meta.profiles` is empty, the legacy `production/` folder exists, the
    /// vertical DB is present, has at least one admin user AND a non-empty
    /// `shop_settings.shop_name`. Eligible for the silent legacy upgrade
    /// in A1.2.
    LegacyCompleted {
        vertical_db_path: PathBuf,
        shop_name: String,
    },

    /// Legacy `production/` folder exists but was never finished:
    /// either the vertical DB file is missing entirely
    /// ([`AbandonReason::NoVerticalDbFile`]) or it's present but no admin
    /// user was ever created ([`AbandonReason::NoAdminUser`]). Treat the
    /// boot as a fresh install — the operator did not get past wizard step 0.
    LegacyAbandoned { reason: AbandonReason },

    /// Legacy `production/` folder exists with admin user(s) present BUT
    /// `shop_settings.shop_name` is empty. Refuse to boot — slug derivation
    /// would produce an empty string. Defensive case; protects against
    /// auto-deriving an empty slug into `meta.profiles`.
    LegacyDefensiveEmpty { vertical_db_path: PathBuf },
}

/// I/O / inspection errors surfaced by [`detect_boot_state`].
#[derive(Debug, Error)]
pub enum BootStateDetectionError {
    #[error("data directory I/O error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not query meta.profiles: {0}")]
    QueryMetaProfiles(#[source] sea_orm::DbErr),

    #[error("could not open legacy vertical DB at {path:?}: {source}")]
    OpenVerticalDb {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("could not query legacy vertical DB at {path:?}: {source}")]
    QueryVerticalDb {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
}

/// Inspect `<data_dir>` and return the boot state.
///
/// `meta_pool` is the already-migrated meta DB pool; the function only reads
/// from `meta.profiles`. `vertical_db_filename` is supplied by the binary
/// (via `Graft::db_filename`) so that this module stays free of vertical
/// vocabulary per invariant I1/strict.
pub async fn detect_boot_state(
    data_dir: &Path,
    meta_pool: &DatabaseConnection,
    vertical_db_filename: &str,
) -> Result<BootState, BootStateDetectionError> {
    let profile_count = count_meta_profiles(meta_pool).await?;
    if profile_count > 0 {
        return Ok(BootState::PostUpgradeOrSetup { profile_count });
    }

    let production_dir = data_dir.join("production");
    match production_dir.try_exists() {
        Ok(false) => return Ok(BootState::FreshInstall),
        Err(source) => {
            return Err(BootStateDetectionError::Io {
                path: production_dir,
                source,
            });
        }
        Ok(true) => {}
    }

    let vertical_db_path = production_dir.join(vertical_db_filename);
    match vertical_db_path.try_exists() {
        Ok(false) => {
            return Ok(BootState::LegacyAbandoned {
                reason: AbandonReason::NoVerticalDbFile,
            });
        }
        Err(source) => {
            return Err(BootStateDetectionError::Io {
                path: vertical_db_path,
                source,
            });
        }
        Ok(true) => {}
    }

    // rusqlite is sync; hop onto the blocking pool so probing a slow legacy
    // disk doesn't stall other boot tasks. join() returns a JoinError only
    // when the thread panics, so unwrap() forwards an unexpected panic and
    // never silently swallows a detection failure.
    tokio::task::spawn_blocking(move || inspect_legacy_vertical_db(vertical_db_path))
        .await
        .expect("inspect_legacy_vertical_db panicked")
}

async fn count_meta_profiles(
    meta_pool: &DatabaseConnection,
) -> Result<usize, BootStateDetectionError> {
    let count = profile::Entity::find()
        .count(meta_pool)
        .await
        .map_err(BootStateDetectionError::QueryMetaProfiles)?;
    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

fn inspect_legacy_vertical_db(
    vertical_db_path: PathBuf,
) -> Result<BootState, BootStateDetectionError> {
    let conn = rusqlite::Connection::open_with_flags(
        &vertical_db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| BootStateDetectionError::OpenVerticalDb {
        path: vertical_db_path.clone(),
        source,
    })?;

    let admin_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM users \
             WHERE role_id = 1 AND is_active = 1 AND deleted_at IS NULL",
            [],
            |row| row.get(0),
        )
        .map_err(|source| BootStateDetectionError::QueryVerticalDb {
            path: vertical_db_path.clone(),
            source,
        })?;

    if admin_count == 0 {
        return Ok(BootState::LegacyAbandoned {
            reason: AbandonReason::NoAdminUser,
        });
    }

    let shop_name: String = conn
        .query_row(
            "SELECT shop_name FROM shop_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|source| BootStateDetectionError::QueryVerticalDb {
            path: vertical_db_path.clone(),
            source,
        })?;

    if shop_name.trim().is_empty() {
        Ok(BootState::LegacyDefensiveEmpty { vertical_db_path })
    } else {
        Ok(BootState::LegacyCompleted {
            vertical_db_path,
            shop_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database};

    const TEST_DB_FILE: &str = "vertical.db";

    async fn open_meta_pool() -> DatabaseConnection {
        let pool = Database::connect("sqlite::memory:").await.unwrap();
        // Mirrors `m_0001_create_meta_profiles` so SeaORM's
        // `Entity::find().count()` (which projects every column inside a
        // subquery before counting) finds the columns it expects.
        pool.execute_unprepared(
            "CREATE TABLE profiles (
                slug TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL DEFAULT '',
                archived_at TEXT
            )",
        )
        .await
        .unwrap();
        pool
    }

    fn seed_legacy_vertical(dir: &Path, admin: bool, shop_name: &str) -> PathBuf {
        let production = dir.join("production");
        std::fs::create_dir_all(&production).unwrap();
        let vertical = production.join(TEST_DB_FILE);
        let conn = rusqlite::Connection::open(&vertical).unwrap();
        conn.execute_batch(
            "CREATE TABLE roles (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO roles (id, name) VALUES (1, 'Admin');
             CREATE TABLE users (
                 id INTEGER PRIMARY KEY,
                 role_id INTEGER NOT NULL,
                 is_active INTEGER NOT NULL DEFAULT 1,
                 deleted_at TEXT
             );
             CREATE TABLE shop_settings (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 shop_name TEXT NOT NULL DEFAULT ''
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO shop_settings (id, shop_name) VALUES (1, ?1)",
            rusqlite::params![shop_name],
        )
        .unwrap();
        if admin {
            conn.execute(
                "INSERT INTO users (role_id, is_active, deleted_at) VALUES (1, 1, NULL)",
                [],
            )
            .unwrap();
        }
        vertical
    }

    #[tokio::test]
    async fn fresh_install_when_no_meta_rows_and_no_production_dir() {
        let dir = tempfile::tempdir().unwrap();
        let pool = open_meta_pool().await;
        let state = detect_boot_state(dir.path(), &pool, TEST_DB_FILE)
            .await
            .unwrap();
        assert_eq!(state, BootState::FreshInstall);
    }

    #[tokio::test]
    async fn post_upgrade_when_meta_profiles_has_rows() {
        let dir = tempfile::tempdir().unwrap();
        let pool = open_meta_pool().await;
        pool.execute_unprepared(
            "INSERT INTO profiles (slug, display_name, kind) VALUES \
             ('demo', 'Demo', 'demo'), ('acme-printing', 'Acme Printing', 'production')",
        )
        .await
        .unwrap();
        let state = detect_boot_state(dir.path(), &pool, TEST_DB_FILE)
            .await
            .unwrap();
        assert_eq!(state, BootState::PostUpgradeOrSetup { profile_count: 2 });
    }

    #[tokio::test]
    async fn legacy_abandoned_when_production_dir_lacks_vertical_db() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("production")).unwrap();
        let pool = open_meta_pool().await;
        let state = detect_boot_state(dir.path(), &pool, TEST_DB_FILE)
            .await
            .unwrap();
        assert_eq!(
            state,
            BootState::LegacyAbandoned {
                reason: AbandonReason::NoVerticalDbFile,
            }
        );
    }

    #[tokio::test]
    async fn legacy_abandoned_when_vertical_db_has_no_admin_user() {
        let dir = tempfile::tempdir().unwrap();
        seed_legacy_vertical(dir.path(), false, "Acme Printing");
        let pool = open_meta_pool().await;
        let state = detect_boot_state(dir.path(), &pool, TEST_DB_FILE)
            .await
            .unwrap();
        assert_eq!(
            state,
            BootState::LegacyAbandoned {
                reason: AbandonReason::NoAdminUser,
            }
        );
    }

    #[tokio::test]
    async fn legacy_defensive_empty_when_admin_present_but_shop_name_blank() {
        let dir = tempfile::tempdir().unwrap();
        let vertical = seed_legacy_vertical(dir.path(), true, "   ");
        let pool = open_meta_pool().await;
        let state = detect_boot_state(dir.path(), &pool, TEST_DB_FILE)
            .await
            .unwrap();
        assert_eq!(
            state,
            BootState::LegacyDefensiveEmpty {
                vertical_db_path: vertical,
            }
        );
    }

    #[tokio::test]
    async fn legacy_completed_when_admin_present_and_shop_name_set() {
        let dir = tempfile::tempdir().unwrap();
        let vertical = seed_legacy_vertical(dir.path(), true, "Acme Printing");
        let pool = open_meta_pool().await;
        let state = detect_boot_state(dir.path(), &pool, TEST_DB_FILE)
            .await
            .unwrap();
        assert_eq!(
            state,
            BootState::LegacyCompleted {
                vertical_db_path: vertical,
                shop_name: "Acme Printing".to_string(),
            }
        );
    }
}
