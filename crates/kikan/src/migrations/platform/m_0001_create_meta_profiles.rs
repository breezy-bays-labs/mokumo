//! M00 platform migration: install-level `meta.profiles` table.
//!
//! Each row is one tenant in kikan's multi-profile model: `slug` is the
//! kebab-case primary key (also the on-disk profile directory name and
//! the lookup key for `PlatformState::pools`). `kind` is the
//! vertical-supplied profile-kind string (matches `Graft::ProfileKind`'s
//! `Display` form); kikan stores it opaquely. `archived_at` enables soft
//! archive (NULL = active, set = archived; hard-delete removes the row
//! entirely). See `adr-kikan-upgrade-migration-strategy.md` for the
//! engine-vs-vertical migration partition that makes this a `Meta` target.

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct CreateMetaProfiles;

#[async_trait::async_trait]
impl Migration for CreateMetaProfiles {
    fn name(&self) -> &'static str {
        "m20260425_000000_create_meta_profiles"
    }

    fn graft_id(&self) -> GraftId {
        PlatformMigrations::graft_id()
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::Meta
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        Vec::new()
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        conn.execute_unprepared(
            "CREATE TABLE profiles (
                slug TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                archived_at TEXT
            )",
        )
        .await?;

        // Partial index supporting "list active profiles" scans without
        // touching archived rows. The PK already covers point lookups by
        // slug; this index lets `WHERE archived_at IS NULL ORDER BY slug`
        // run as an index-only scan.
        conn.execute_unprepared(
            "CREATE INDEX idx_profiles_active_slug ON profiles(slug) WHERE archived_at IS NULL",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE TRIGGER profiles_updated_at
                AFTER UPDATE ON profiles
                FOR EACH ROW
                WHEN NEW.updated_at = OLD.updated_at
                BEGIN
                    UPDATE profiles
                       SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                     WHERE slug = OLD.slug;
                END",
        )
        .await?;

        Ok(())
    }
}
