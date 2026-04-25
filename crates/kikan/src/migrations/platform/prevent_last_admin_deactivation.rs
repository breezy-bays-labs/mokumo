//! M00 platform migration: refuse to deactivate the last active install Admin.
//!
//! Install-level Admin count must stay ≥ 1. Without this trigger, a
//! mis-click in the Users screen ("Deactivate") on the sole Admin would
//! lock every operator out of the admin surface; the only recovery would
//! be a DB edit or a full reinstall.
//!
//! Enforced at the DB layer (not just the API) to make the invariant
//! transport-independent — CLI tools and direct SQL sessions cannot
//! bypass the guard. The UI still surfaces the same invariant with a
//! toast to prevent the user from ever seeing the raw trigger error.
//!
//! See `ops/decisions/mokumo/adr-kikan-admin-ui.md` §ADR-5 for the role
//! model this guard enforces.

use crate::migrations::conn::MigrationConn;
use crate::migrations::{GraftId, Migration, MigrationRef, MigrationTarget};

use super::PlatformMigrations;

pub(crate) struct PreventLastAdminDeactivation;

#[async_trait::async_trait]
impl Migration for PreventLastAdminDeactivation {
    fn name(&self) -> &'static str {
        "m20260424_000001_prevent_last_admin_deactivation"
    }

    fn graft_id(&self) -> GraftId {
        PlatformMigrations::graft_id()
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::PerProfile
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        vec![
            MigrationRef {
                graft: PlatformMigrations::graft_id(),
                name: "m20260327_000000_users_and_roles",
            },
            MigrationRef {
                graft: PlatformMigrations::graft_id(),
                name: "m20260424_000000_profile_user_roles",
            },
        ]
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        // Covering index for the trigger's `COUNT(*) FROM users
        // WHERE role_id = 1 AND is_active = 1 AND deleted_at IS NULL`
        // lookup. Without this, every admin deactivation/demotion would
        // scan the full `users` table.
        conn.execute_unprepared(
            "CREATE INDEX idx_users_active_admins
                ON users(role_id, is_active)
                WHERE deleted_at IS NULL",
        )
        .await?;

        // "Admin" here is the install-level role — `users.role_id = 1` per
        // the `roles` table seeded in users_and_roles. The trigger RAISEs
        // ABORT on deactivation when the target row is the only active
        // Admin left.
        //
        // Runs on UPDATE of `is_active` (the direct deactivate path) and
        // on UPDATE of `role_id` (the "demote the last Admin" path). Both
        // SHOULD be prevented. Soft-delete via `deleted_at` is covered by
        // the `is_active` path because the application sets `is_active=0`
        // when soft-deleting; a separate UPDATE-of-`deleted_at` branch
        // would be defense-in-depth but is intentionally omitted here to
        // keep the trigger focused. A follow-up can add it if operator
        // error patterns warrant.
        // Trigger WHEN clauses include `OLD.deleted_at IS NULL` so the
        // gating predicate matches the COUNT subquery's filter — a
        // soft-deleted row (the inconsistent case where deleted_at is
        // set but is_active is still 1) is excluded from both sides.
        conn.execute_unprepared(
            "CREATE TRIGGER users_last_admin_deactivation_guard
                BEFORE UPDATE OF is_active ON users
                FOR EACH ROW
                WHEN OLD.role_id = 1 AND OLD.is_active = 1 AND NEW.is_active = 0
                     AND OLD.deleted_at IS NULL
                BEGIN
                    SELECT CASE
                        WHEN (SELECT COUNT(*) FROM users
                              WHERE role_id = 1 AND is_active = 1 AND deleted_at IS NULL) <= 1
                        THEN RAISE(ABORT, 'cannot deactivate the last active install Admin')
                    END;
                END",
        )
        .await?;

        conn.execute_unprepared(
            "CREATE TRIGGER users_last_admin_demote_guard
                BEFORE UPDATE OF role_id ON users
                FOR EACH ROW
                WHEN OLD.role_id = 1 AND NEW.role_id != 1 AND OLD.is_active = 1
                     AND OLD.deleted_at IS NULL
                BEGIN
                    SELECT CASE
                        WHEN (SELECT COUNT(*) FROM users
                              WHERE role_id = 1 AND is_active = 1 AND deleted_at IS NULL) <= 1
                        THEN RAISE(ABORT, 'cannot demote the last active install Admin')
                    END;
                END",
        )
        .await?;

        Ok(())
    }
}
