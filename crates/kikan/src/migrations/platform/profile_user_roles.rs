//! M00 platform migration: install-level `(profile_id, user_id, role)` mapping.
//!
//! Introduces the three-tier permission model for Kikan admin UI: a user
//! may be a Profile Admin or Profile User on each profile they belong to,
//! independent of their install-level role in the `users` table. Install
//! Owner / Admin / None is expressed elsewhere; this table captures the
//! profile-level dimension.
//!
//! See `ops/decisions/mokumo/adr-kikan-admin-ui.md` §ADR-5 for the role
//! model and the three-tier dispatch rules.

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct ProfileUserRoles;

#[async_trait::async_trait]
impl Migration for ProfileUserRoles {
    fn name(&self) -> &'static str {
        "m20260424_000000_profile_user_roles"
    }

    fn graft_id(&self) -> GraftId {
        PlatformMigrations::graft_id()
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::Meta
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        // Both halves of `(profile_id, user_id)` resolve to in-DB tables on
        // `meta.db`: `profile_id` references `profiles(slug)` and `user_id`
        // references `users(id)`.
        vec![
            MigrationRef {
                graft: PlatformMigrations::graft_id(),
                name: "m20260425_000000_create_meta_profiles",
            },
            MigrationRef {
                graft: PlatformMigrations::graft_id(),
                name: "m20260327_000000_users_and_roles",
            },
        ]
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        // `role` is stored as TEXT with a CHECK constraint rather than a
        // reference to a roles table — the role enum has only two variants
        // (Admin, User) and is unlikely to grow. A TEXT+CHECK column keeps
        // the migration self-contained and matches the existing SQLite
        // idiom used elsewhere in the platform DB.
        conn.execute_unprepared(
            "CREATE TABLE profile_user_roles (
                profile_id TEXT NOT NULL REFERENCES profiles(slug) ON DELETE CASCADE,
                user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role TEXT NOT NULL CHECK (role IN ('Admin', 'User')),
                granted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                PRIMARY KEY (profile_id, user_id)
            )",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE INDEX idx_profile_user_roles_user_id ON profile_user_roles(user_id)",
        )
        .await?;

        Ok(())
    }
}
