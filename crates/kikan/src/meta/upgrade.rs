//! Legacy install upgrade — silently records a `production/` install in
//! `meta.profiles` so the runtime registry knows about it.
//!
//! # Scope (PR A — Meta-only)
//!
//! This handler runs once per install when [`detect_boot_state`] returns
//! [`BootState::LegacyCompleted`]. It:
//!
//! 1. Derives a kebab-case slug from the legacy `shop_settings.shop_name`.
//! 2. INSERTs a row into `meta.profiles` and a row into `meta.activity_log`
//!    inside a single transaction on the meta DB.
//!
//! It does **not** rename the `production/` directory and it does **not**
//! update the `<data_dir>/active_profile` pointer. The binary's
//! `prepare_database` and the engine's pool map continue to address the
//! legacy install as `production` until PR B refactors those call sites
//! to consult `meta.profiles`. `meta.profiles` is therefore "shadow truth"
//! in PR A and becomes "physical truth" in PR B.
//!
//! Idempotency is provided by the caller, not this function: on the next
//! boot, `meta.profiles` will have one row, so [`detect_boot_state`]
//! returns [`BootState::PostUpgradeOrSetup`] and the upgrade arm is never
//! re-entered.
//!
//! # Audit
//!
//! The activity log entry uses [`ActivityAction::LegacyUpgradeMigrated`]
//! and lands in `meta.activity_log` (created by
//! `m_0003_create_meta_activity_log`). Its payload carries the original
//! `shop_name` and the legacy vertical DB path so an operator can correlate
//! the audit row with the on-disk layout.
//!
//! [`detect_boot_state`]: crate::meta::detect_boot_state
//! [`BootState::LegacyCompleted`]: crate::meta::BootState::LegacyCompleted
//! [`BootState::PostUpgradeOrSetup`]: crate::meta::BootState::PostUpgradeOrSetup
//! [`ActivityAction::LegacyUpgradeMigrated`]: kikan_types::activity::ActivityAction::LegacyUpgradeMigrated

use std::collections::HashSet;
use std::hash::Hash;
use std::path::Path;

use kikan_types::activity::ActivityAction;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, OnConflict, Query, SqliteQueryBuilder, Value};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, Statement,
    TransactionTrait, TryGetable,
};
use serde_json::json;
use thiserror::Error;

use crate::activity::insert_activity_log_raw;
use crate::slug::{Slug, SlugError, derive_slug};

/// Errors surfaced by [`run_legacy_upgrade`].
#[derive(Debug, Error)]
pub enum UpgradeError {
    /// `derive_slug(shop_name)` rejected the input. Wraps the specific
    /// rule violated (Empty / Reserved / TooLong / Unparseable).
    #[error("legacy upgrade rejected shop_name `{shop_name}`: {source}")]
    SlugDerivation {
        shop_name: String,
        #[source]
        source: SlugError,
    },

    /// SeaORM error while writing meta.profiles or meta.activity_log, or
    /// while opening / committing the transaction.
    #[error("legacy upgrade DB error: {0}")]
    Db(#[from] sea_orm::DbErr),

    /// SQLx error while running pre-flight intersection or the user/role
    /// data move against the per-profile or meta pool.
    #[error("legacy upgrade SQLx error: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// `insert_activity_log_raw` failed (e.g. payload serialization). Bubbles
    /// up the underlying domain error so the caller sees the same surface as
    /// other activity-log call sites.
    #[error("legacy upgrade activity-log write failed: {0}")]
    ActivityLog(#[from] crate::error::DomainError),

    /// Pre-flight detected an unsupported state that would risk data loss
    /// or privilege escalation if the upgrade proceeded — multi-legacy-
    /// profile collisions on user IDs or custom role IDs (where the
    /// PR-A→main upgrade path supports only one legacy profile), or an
    /// anomalous partial intersection in `meta.users` indicating external
    /// mutation between the prior run's commit and drop phases.
    ///
    /// The upgrade aborts before mutating any data; the install is left
    /// in exactly its pre-upgrade condition for backup-restore.
    #[error("legacy upgrade refused: {0}")]
    UnsupportedLegacyState(String),
}

/// Outcome of a successful upgrade — exposed so the caller can log the
/// derived slug without re-deriving it.
#[derive(Debug, Clone)]
pub struct UpgradeOutcome {
    pub slug: Slug,
}

/// Insert a `meta.profiles` row + a `meta.activity_log` audit entry for a
/// legacy `production/` install detected at boot, after migrating the
/// pre-PR-A per-profile `users` + `roles` tables into `meta.users` /
/// `meta.roles`.
///
/// `kind` is the production-equivalent profile-kind string (the caller
/// reads it from `Graft::auth_profile_kind`'s `Display`). Kikan stores it
/// opaquely; the vertical owns the vocabulary.
///
/// # Data-move state machine
///
/// Pre-PR-A per-profile DBs hold `users` and `roles` tables. The current
/// platform schema places those tables on `meta.db`. The upgrade observes
/// the physical state on each boot and chooses one of three branches:
///
/// - **State A** (legacy tables exist, none of their emails in `meta.users`):
///   pre-flight role-id and user-id collision checks run against the same
///   `BEGIN IMMEDIATE` meta transaction as the inserts (TOCTOU-safe);
///   inserts run; meta tx commits; legacy tables drop in a separate tx.
/// - **State B** (legacy tables exist, all of their emails already in
///   `meta.users`): a prior run committed the meta inserts but crashed
///   before the legacy drop. Skip the inserts and drop only — completing
///   the upgrade.
/// - **State C** (legacy tables already gone): no-op.
///
/// Partial intersections (some but not all legacy emails present) are
/// classified as Anomalous and abort with [`UpgradeError::UnsupportedLegacyState`]
/// without mutating any data. This indicates external mutation of `meta.db`
/// between commit and drop and warrants a backup-restore.
///
/// Multi-legacy-profile installs (a true ID collision in State A, where
/// another source already populated `meta.users` or custom roles) are not
/// supported by the PR-A→main upgrade path; they also abort in State A's
/// collision pre-flight before any meta mutation.
pub async fn run_legacy_upgrade(
    meta_db: &DatabaseConnection,
    auth_pool: &DatabaseConnection,
    shop_name: &str,
    vertical_db_path: &Path,
    kind: &str,
) -> Result<UpgradeOutcome, UpgradeError> {
    // Fast pre-flight before touching any DB: a malformed shop_name is a
    // pure validation failure — no point starting the data move only to
    // fail on slug derivation at the end.
    let slug = derive_slug(shop_name).map_err(|source| UpgradeError::SlugDerivation {
        shop_name: shop_name.to_owned(),
        source,
    })?;

    // ── 1. Data move (state machine) ─────────────────────────────────
    //
    // Runs first so that an Unsupported / Anomalous abort leaves the
    // install in exactly its pre-upgrade condition. The existing
    // meta.profiles + activity_log inserts below are idempotent across
    // crash points: if the data move commits and then a crash interrupts
    // the meta.profiles insert, the next boot's `detect_boot_state` still
    // sees `meta.profiles` empty and re-enters the upgrade — the state
    // machine observes State C (legacy tables gone) and falls through to
    // re-attempt the profiles insert cleanly.
    run_data_move(meta_db, auth_pool).await?;

    // ── 2. meta.profiles + meta.activity_log inserts (single sea_orm tx) ──
    let txn = meta_db.begin().await?;

    txn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO profiles (slug, display_name, kind) VALUES (?, ?, ?)",
        [slug.as_str().into(), shop_name.into(), kind.into()],
    ))
    .await?;

    let payload = json!({
        "shop_name": shop_name,
        "vertical_db_path": vertical_db_path.display().to_string(),
        "kind": kind,
    });
    insert_activity_log_raw(
        &txn,
        "profile",
        slug.as_str(),
        ActivityAction::LegacyUpgradeMigrated,
        "system",
        "system",
        &payload,
    )
    .await?;

    txn.commit().await?;

    Ok(UpgradeOutcome { slug })
}

#[derive(sqlx::FromRow, Debug)]
struct LegacyUserRow {
    id: i64,
    email: String,
    name: String,
    password_hash: String,
    role_id: i64,
    is_active: bool,
    last_login_at: Option<String>,
    recovery_code_hash: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
}

#[derive(sqlx::FromRow, Debug)]
struct LegacyRoleRow {
    id: i64,
    name: String,
    description: Option<String>,
    created_at: String,
}

async fn run_data_move(
    meta_db: &DatabaseConnection,
    auth_pool: &DatabaseConnection,
) -> Result<(), UpgradeError> {
    // State C check on the auth pool — legacy tables already gone. This
    // read carries no TOCTOU concern (the auth pool is the per-profile
    // pool, exclusive to this profile under the single-Engine-per-data-
    // dir contract), so it runs outside any transaction.
    let users_table_present: i64 = auth_pool
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'",
        ))
        .await?
        .map_or(0, |r| r.try_get_by_index::<i64>(0).unwrap_or(0));

    if users_table_present == 0 {
        tracing::info!("legacy upgrade data-move: state=C (legacy users table absent, no-op)");
        return Ok(());
    }

    // Read legacy data with explicit projections — never SELECT *. The
    // schema diff against pre-Stage-3 fixtures (commit cdf4df3 vs. current
    // platform `users_and_roles`) confirmed these column lists are byte-
    // identical; if they ever diverge, the explicit projection forces the
    // mismatch into a runtime error rather than silent data drift.
    //
    // Auth-pool reads use `sqlx::query_as` per the hybrid ORM convention
    // (coding-standards #3): no TOCTOU surface here, no Entity types
    // worth defining for a one-shot read, and `#[derive(FromRow)]` keeps
    // the projection columns in lockstep with the row struct.
    let auth_sqlx = auth_pool.get_sqlite_connection_pool();
    let legacy_users: Vec<LegacyUserRow> = sqlx::query_as(
        "SELECT id, email, name, password_hash, role_id, is_active, \
                last_login_at, recovery_code_hash, created_at, updated_at, deleted_at \
         FROM users",
    )
    .fetch_all(auth_sqlx)
    .await?;

    let legacy_roles: Vec<LegacyRoleRow> =
        sqlx::query_as("SELECT id, name, description, created_at FROM roles")
            .fetch_all(auth_sqlx)
            .await?;

    // Open a SeaORM transaction on meta. Drop semantics auto-rollback on
    // any error path that doesn't explicitly `commit()` — this is the
    // safety net we lose if we go around SeaORM with raw `BEGIN IMMEDIATE`
    // on a pooled sqlx connection.
    //
    // SeaORM's default `begin()` uses SQLite `BEGIN DEFERRED`, which
    // would lazily upgrade to RESERVED on first write — leaving a window
    // between our SELECT-based state classification and the first INSERT
    // where a concurrent writer could intervene. To close that window we
    // immediately upsert into `meta.legacy_upgrade_locks` (singleton
    // row), which forces SQLite to acquire RESERVED at this point. See
    // `m_0004_create_legacy_upgrade_locks` for why a dedicated table is
    // used rather than a `WHERE 1=0` no-op write (which the optimizer
    // can elide).
    let txn = meta_db.begin().await?;

    txn.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO legacy_upgrade_locks (id, locked_at) \
         VALUES (1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
         ON CONFLICT(id) DO UPDATE SET locked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    ))
    .await?;

    let action = classify_and_maybe_insert(&txn, &legacy_users, &legacy_roles).await?;

    match action {
        DataMoveAction::CommitMetaThenDrop => {
            txn.commit().await?;
            drop_legacy_tables(auth_pool).await?;
            tracing::info!("legacy upgrade data-move: state=A (fresh, committed + dropped)");
        }
        DataMoveAction::RollbackMetaThenDrop => {
            // No meta state to commit (other than the lock-row upsert,
            // which is intentionally rolled back so the audit timestamp
            // reflects only successful upgrades / commits).
            txn.rollback().await?;
            drop_legacy_tables(auth_pool).await?;
            tracing::info!("legacy upgrade data-move: state=B (crash recovery, drop only)");
        }
    }

    Ok(())
}

#[derive(Debug)]
enum DataMoveAction {
    CommitMetaThenDrop,
    RollbackMetaThenDrop,
}

async fn classify_and_maybe_insert(
    txn: &DatabaseTransaction,
    legacy_users: &[LegacyUserRow],
    legacy_roles: &[LegacyRoleRow],
) -> Result<DataMoveAction, UpgradeError> {
    // Email-intersection check — single source of truth for state
    // classification. This is what distinguishes State A (fresh) from
    // State B (crash recovery) from Anomalous (partial mutation).
    let legacy_emails: Vec<String> = legacy_users.iter().map(|u| u.email.clone()).collect();
    let present_emails: HashSet<String> =
        intersect_chunked(txn, "users", "email", &legacy_emails).await?;

    let n_legacy = legacy_emails.len();
    let n_present = present_emails.len();

    // Branch ordering matters: empty-legacy AND none-present both route to
    // State A, but the order is "n_legacy == 0 || n_present == 0 first" so
    // an empty legacy users table doesn't get classified as Anomalous.
    if n_legacy == 0 || n_present == 0 {
        // State A — fresh upgrade. Now run collision pre-flights inside
        // the same transaction. Intentionally AFTER state classification:
        // running them before would cause a State B crash-recovery boot
        // to mis-detect "collisions" against rows that we ourselves
        // committed last run, and falsely abort.
        let custom_role_ids: Vec<i64> = legacy_roles
            .iter()
            .map(|r| r.id)
            .filter(|&id| id > 3)
            .collect();
        let role_collisions: HashSet<i64> =
            intersect_chunked(txn, "roles", "id", &custom_role_ids).await?;
        if !role_collisions.is_empty() {
            let mut sorted: Vec<i64> = role_collisions.into_iter().collect();
            sorted.sort_unstable();
            return Err(UpgradeError::UnsupportedLegacyState(format!(
                "legacy custom role IDs {sorted:?} are already present in meta.roles \
                 from another source. Proceeding would cause legacy users to silently \
                 inherit a different role's permissions (privilege escalation risk). \
                 Multi-legacy-profile installs are not supported by the PR-A → main \
                 upgrade path. Restore from the pre-upgrade backup and contact \
                 support before retrying."
            )));
        }

        let user_ids: Vec<i64> = legacy_users.iter().map(|u| u.id).collect();
        let user_collisions: HashSet<i64> =
            intersect_chunked(txn, "users", "id", &user_ids).await?;
        if !user_collisions.is_empty() {
            let mut sorted: Vec<i64> = user_collisions.into_iter().collect();
            sorted.sort_unstable();
            return Err(UpgradeError::UnsupportedLegacyState(format!(
                "legacy user IDs {sorted:?} are already present in meta.users from \
                 another source. Multi-legacy-profile installs are not supported by \
                 the PR-A → main upgrade path. Restore from the pre-upgrade backup \
                 and contact support before retrying."
            )));
        }

        insert_legacy_into_meta(txn, legacy_roles, legacy_users).await?;
        Ok(DataMoveAction::CommitMetaThenDrop)
    } else if n_present == n_legacy {
        // State B — crash recovery. Skip inserts, drop only. Collision
        // checks intentionally NOT run here: by construction those
        // "collisions" are our own previously-committed rows, and running
        // the check would falsely brick the recovery path.
        Ok(DataMoveAction::RollbackMetaThenDrop)
    } else {
        // Anomalous — partial intersection. The meta-side insert is
        // atomic, so this cannot arise from our own crash; it indicates
        // external mutation of meta.users between the prior run's commit
        // and drop phases.
        Err(UpgradeError::UnsupportedLegacyState(format!(
            "{n_present} of {n_legacy} legacy emails already present in meta.users. \
             This indicates external mutation of meta.db between the upgrade's \
             insert and drop phases. Restore from the pre-upgrade backup and \
             contact support."
        )))
    }
}

/// Chunked `IN`-clause intersection. Returns the subset of `needles`
/// present in `<table>.<column>` on the meta DB, using sea_query's
/// [`Expr::is_in`] for parameter binding.
///
/// Chunks at 900 bindings per query to stay safely under SQLite's
/// `SQLITE_MAX_VARIABLE_NUMBER` (historically 999, raised to 32766 in
/// newer builds — 900 is the conservative floor that works against any
/// libsqlite3 version we might link).
///
/// Generic over `T: Into<Value> + TryGetable + Hash + Eq + Clone` —
/// the standard SeaORM idiom for "anything that can both bind into a
/// statement and decode out of a row". `String` and `i64` are the only
/// in-tree call sites; `&'static str` would also work if a column ever
/// needed it.
async fn intersect_chunked<T>(
    txn: &DatabaseTransaction,
    table: &'static str,
    column: &'static str,
    needles: &[T],
) -> Result<HashSet<T>, UpgradeError>
where
    T: Clone + Hash + Eq + Send + Sync + Into<Value> + TryGetable + 'static,
{
    const CHUNK: usize = 900;
    let mut found: HashSet<T> = HashSet::with_capacity(needles.len());
    if needles.is_empty() {
        return Ok(found);
    }
    for chunk in needles.chunks(CHUNK) {
        let stmt = Query::select()
            .column(Alias::new(column))
            .from(Alias::new(table))
            .and_where(Expr::col(Alias::new(column)).is_in(chunk.iter().cloned().map(Into::into)))
            .to_owned();
        let (sql, params) = stmt.build(SqliteQueryBuilder);
        let rows = txn
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                params,
            ))
            .await?;
        for r in rows {
            found.insert(r.try_get_by_index::<T>(0)?);
        }
    }
    Ok(found)
}

async fn insert_legacy_into_meta(
    txn: &DatabaseTransaction,
    legacy_roles: &[LegacyRoleRow],
    legacy_users: &[LegacyUserRow],
) -> Result<(), UpgradeError> {
    // Roles: INSERT OR IGNORE (sea_query: OnConflict::do_nothing) so the
    // platform-seeded rows (id 1=Admin, 2=Staff, 3=Guest) win on conflict.
    // Custom legacy roles (id > 3) flow through — the State-A collision
    // check above guarantees no foreign custom role IDs are present.
    for r in legacy_roles {
        let stmt = Query::insert()
            .into_table(Alias::new("roles"))
            .columns([
                Alias::new("id"),
                Alias::new("name"),
                Alias::new("description"),
                Alias::new("created_at"),
            ])
            .values_panic([
                r.id.into(),
                r.name.as_str().into(),
                r.description.as_deref().into(),
                r.created_at.as_str().into(),
            ])
            .on_conflict(OnConflict::column(Alias::new("id")).do_nothing().to_owned())
            .to_owned();
        let (sql, params) = stmt.build(SqliteQueryBuilder);
        txn.execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            params,
        ))
        .await?;
    }

    // Users: explicit columns, preserve legacy IDs (the State-A collision
    // check guarantees no ID conflicts; no ON CONFLICT here so a hidden
    // collision surfaces as a hard error rather than silent skip).
    for u in legacy_users {
        let stmt = Query::insert()
            .into_table(Alias::new("users"))
            .columns([
                Alias::new("id"),
                Alias::new("email"),
                Alias::new("name"),
                Alias::new("password_hash"),
                Alias::new("role_id"),
                Alias::new("is_active"),
                Alias::new("last_login_at"),
                Alias::new("recovery_code_hash"),
                Alias::new("created_at"),
                Alias::new("updated_at"),
                Alias::new("deleted_at"),
            ])
            .values_panic([
                u.id.into(),
                u.email.as_str().into(),
                u.name.as_str().into(),
                u.password_hash.as_str().into(),
                u.role_id.into(),
                u.is_active.into(),
                u.last_login_at.as_deref().into(),
                u.recovery_code_hash.as_deref().into(),
                u.created_at.as_str().into(),
                u.updated_at.as_str().into(),
                u.deleted_at.as_deref().into(),
            ])
            .to_owned();
        let (sql, params) = stmt.build(SqliteQueryBuilder);
        txn.execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            params,
        ))
        .await?;
    }

    Ok(())
}

async fn drop_legacy_tables(auth_pool: &DatabaseConnection) -> Result<(), UpgradeError> {
    // Separate SeaORM transaction on the per-profile pool. The auto-
    // rollback-on-drop semantics catch any partial-failure path. State C
    // re-entry is naturally idempotent — `DROP IF EXISTS` is a no-op when
    // the artefacts are already gone.
    //
    // DDL statements live as raw `Statement::from_string` because
    // sea_query's builder surface doesn't cover `DROP TRIGGER` /
    // `DROP TABLE IF EXISTS`. The literals are static strings — no SQL
    // injection surface.
    let txn = auth_pool.begin().await?;

    txn.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "DROP TRIGGER IF EXISTS users_updated_at",
    ))
    .await?;
    txn.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "DROP INDEX IF EXISTS idx_users_deleted_at",
    ))
    .await?;
    txn.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "DROP TABLE IF EXISTS users",
    ))
    .await?;
    txn.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "DROP TABLE IF EXISTS roles",
    ))
    .await?;

    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::platform::run_platform_meta_migrations;
    use sea_orm::{ConnectionTrait, Database};

    async fn meta_pool() -> DatabaseConnection {
        let pool = Database::connect("sqlite::memory:").await.unwrap();
        run_platform_meta_migrations(&pool).await.unwrap();
        pool
    }

    /// Build an in-memory per-profile pool with no `users` / `roles`
    /// tables. The state machine sees this as State C and falls through
    /// to the existing meta inserts unchanged — used by the slug + audit
    /// tests that don't exercise the data-move surface.
    async fn empty_auth_pool() -> DatabaseConnection {
        Database::connect("sqlite::memory:").await.unwrap()
    }

    /// Build an in-memory per-profile pool seeded with the pre-Stage-3
    /// `users` + `roles` schema and a small fixture (one admin user, the
    /// 3 default roles). Mirrors what `tests/fixtures/pre-stage3.sqlite`
    /// physically contains.
    async fn pre_stage3_auth_pool() -> DatabaseConnection {
        let pool = Database::connect("sqlite::memory:").await.unwrap();
        pool.execute_unprepared(
            "CREATE TABLE roles (
                id INTEGER PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                description TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
             INSERT INTO roles (id, name, description) VALUES
                (1, 'Admin', 'Full access to all features'),
                (2, 'Staff', 'Standard staff access'),
                (3, 'Guest', 'Read-only guest access');
             CREATE TABLE users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                role_id INTEGER NOT NULL DEFAULT 1 REFERENCES roles(id) ON DELETE RESTRICT,
                is_active BOOLEAN NOT NULL DEFAULT 1,
                last_login_at TEXT,
                recovery_code_hash TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                deleted_at TEXT
             );
             CREATE INDEX idx_users_deleted_at ON users(id) WHERE deleted_at IS NULL;
             CREATE TRIGGER users_updated_at AFTER UPDATE ON users
                FOR EACH ROW BEGIN
                    UPDATE users SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = OLD.id;
                END;
             INSERT INTO users (id, email, name, password_hash, role_id, is_active,
                                created_at, updated_at)
                VALUES (1, 'admin@example.com', 'Admin', 'hash-1', 1, 1,
                        '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z');",
        )
        .await
        .unwrap();
        pool
    }

    async fn meta_users_count(meta: &DatabaseConnection) -> i64 {
        meta.query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) FROM users",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index::<i64>(0)
        .unwrap()
    }

    async fn legacy_users_table_present(auth: &DatabaseConnection) -> bool {
        let row = auth
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'",
            ))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<i64>(0).unwrap() > 0
    }

    #[tokio::test]
    async fn happy_path_inserts_profile_row_with_derived_slug() {
        let pool = meta_pool().await;
        let auth = empty_auth_pool().await;
        let outcome = run_legacy_upgrade(
            &pool,
            &auth,
            "Acme Printing",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();
        assert_eq!(outcome.slug.as_str(), "acme-printing");

        let rows: Vec<(String, String, String)> = pool
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT slug, display_name, kind FROM profiles",
            ))
            .await
            .unwrap()
            .into_iter()
            .map(|r| {
                (
                    r.try_get_by_index::<String>(0).unwrap(),
                    r.try_get_by_index::<String>(1).unwrap(),
                    r.try_get_by_index::<String>(2).unwrap(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![(
                "acme-printing".to_string(),
                "Acme Printing".to_string(),
                "production".to_string(),
            )]
        );
    }

    #[tokio::test]
    async fn happy_path_writes_activity_log_entry() {
        let pool = meta_pool().await;
        let auth = empty_auth_pool().await;
        run_legacy_upgrade(
            &pool,
            &auth,
            "Acme Printing",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();

        let row = pool
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT entity_type, entity_id, action, actor_id, actor_type, payload \
                 FROM activity_log",
            ))
            .await
            .unwrap()
            .expect("expected an activity_log row");
        assert_eq!(row.try_get_by_index::<String>(0).unwrap(), "profile");
        assert_eq!(row.try_get_by_index::<String>(1).unwrap(), "acme-printing");
        assert_eq!(
            row.try_get_by_index::<String>(2).unwrap(),
            "legacy_upgrade_migrated"
        );
        assert_eq!(row.try_get_by_index::<String>(3).unwrap(), "system");
        assert_eq!(row.try_get_by_index::<String>(4).unwrap(), "system");
        let payload: serde_json::Value =
            serde_json::from_str(&row.try_get_by_index::<String>(5).unwrap()).unwrap();
        assert_eq!(payload["shop_name"], "Acme Printing");
        assert_eq!(payload["vertical_db_path"], "/data/production/mokumo.db");
        assert_eq!(payload["kind"], "production");
    }

    #[tokio::test]
    async fn unparseable_shop_name_returns_slug_derivation_error() {
        let pool = meta_pool().await;
        let auth = empty_auth_pool().await;
        let err = run_legacy_upgrade(
            &pool,
            &auth,
            "!!!",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            UpgradeError::SlugDerivation {
                ref shop_name,
                source: SlugError::Unparseable { .. },
            } if shop_name == "!!!"
        ));
        // Transaction never opened on slug failure → no profile row.
        let count: i64 = pool
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) FROM profiles",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index(0)
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn reserved_slug_returns_slug_derivation_error() {
        let pool = meta_pool().await;
        let auth = empty_auth_pool().await;
        let err = run_legacy_upgrade(
            &pool,
            &auth,
            "Demo",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            UpgradeError::SlugDerivation {
                source: SlugError::Reserved(ref s),
                ..
            } if s == "demo"
        ));
    }

    // ── Data-move state machine ───────────────────────────────────────

    #[tokio::test]
    async fn state_a_fresh_upgrade_migrates_users_and_drops_legacy_tables() {
        let meta = meta_pool().await;
        let auth = pre_stage3_auth_pool().await;

        run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();

        assert_eq!(meta_users_count(&meta).await, 1);
        assert!(!legacy_users_table_present(&auth).await);

        // Roles are correctly seeded by the platform migration; legacy
        // INSERT OR IGNORE means platform seeds win — descriptions match
        // the platform-seeded values, not anything the legacy DB might
        // have customised.
        let roles_count: i64 = meta
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) FROM roles",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index(0)
            .unwrap();
        assert_eq!(roles_count, 3);
    }

    #[tokio::test]
    async fn state_c_no_legacy_tables_is_noop_for_data_move() {
        let meta = meta_pool().await;
        let auth = empty_auth_pool().await;

        run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();

        // No users were migrated; meta.users is empty (the platform
        // migration creates the table but seeds no users).
        assert_eq!(meta_users_count(&meta).await, 0);
    }

    #[tokio::test]
    async fn state_b_crash_recovery_drops_legacy_when_meta_already_has_emails() {
        // Simulate the exact crash scenario: a prior run inserted users
        // into meta but crashed before dropping legacy tables. Next boot
        // must classify as State B, skip the duplicate insert (which would
        // otherwise fail UNIQUE), and drop the legacy tables.
        let meta = meta_pool().await;
        let auth = pre_stage3_auth_pool().await;

        // Pre-populate meta.users with the same email + id as the legacy
        // fixture user. This is what a successful prior run would have
        // committed before crashing.
        meta.execute_unprepared(
            "INSERT INTO users (id, email, name, password_hash, role_id, is_active,
                                created_at, updated_at)
             VALUES (1, 'admin@example.com', 'Admin', 'hash-1', 1, 1,
                     '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
        )
        .await
        .unwrap();

        run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();

        // Meta still has exactly the one user — no duplicate insert.
        assert_eq!(meta_users_count(&meta).await, 1);
        // Legacy tables now dropped — recovery completed.
        assert!(!legacy_users_table_present(&auth).await);
    }

    #[tokio::test]
    async fn state_a_user_id_collision_aborts_before_any_meta_mutation() {
        let meta = meta_pool().await;
        let auth = pre_stage3_auth_pool().await;

        // Seed meta with a foreign user at the same id as our legacy
        // user (id=1) but with a different email — simulates the
        // unsupported multi-legacy-profile case.
        meta.execute_unprepared(
            "INSERT INTO users (id, email, name, password_hash, role_id, is_active,
                                created_at, updated_at)
             VALUES (1, 'someone-else@example.com', 'Someone Else', 'hash-2', 1, 1,
                     '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
        )
        .await
        .unwrap();

        let err = run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap_err();

        match err {
            UpgradeError::UnsupportedLegacyState(msg) => {
                assert!(msg.contains("legacy user IDs"), "got: {msg}");
                assert!(
                    msg.to_lowercase().contains("multi-legacy-profile"),
                    "got: {msg}"
                );
            }
            other => panic!("expected UnsupportedLegacyState, got {other:?}"),
        }

        // No drop happened — legacy data preserved for backup-restore.
        assert!(legacy_users_table_present(&auth).await);
        // Meta is unchanged from its seeded state — exactly 1 row.
        assert_eq!(meta_users_count(&meta).await, 1);
        // No profiles row written either — the data move ran first and
        // aborted before the existing meta.profiles insert.
        let profiles_count: i64 = meta
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) FROM profiles",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index(0)
            .unwrap();
        assert_eq!(profiles_count, 0);
    }

    #[tokio::test]
    async fn state_a_custom_role_id_collision_aborts_with_privilege_warning() {
        let meta = meta_pool().await;
        let auth = pre_stage3_auth_pool().await;

        // Add a custom role at id=4 to the legacy DB.
        sea_orm::ConnectionTrait::execute_unprepared(
            &auth,
            "INSERT INTO roles (id, name, description) VALUES (4, 'Owner', 'Shop owner')",
        )
        .await
        .unwrap();

        // Seed meta.roles with a different role at the same id=4 from a
        // foreign source — simulates the privilege-escalation scenario.
        meta.execute_unprepared(
            "INSERT INTO roles (id, name, description) VALUES (4, 'Suspended', 'Read-only')",
        )
        .await
        .unwrap();

        let err = run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap_err();

        match err {
            UpgradeError::UnsupportedLegacyState(msg) => {
                assert!(msg.contains("custom role IDs"), "got: {msg}");
                assert!(msg.contains("privilege escalation"), "got: {msg}");
            }
            other => panic!("expected UnsupportedLegacyState, got {other:?}"),
        }

        // Legacy tables intact, no users migrated, no profiles row.
        assert!(legacy_users_table_present(&auth).await);
        assert_eq!(meta_users_count(&meta).await, 0);
    }

    #[tokio::test]
    async fn state_a_then_second_boot_observes_state_c_no_op() {
        // Idempotency: a successful boot followed by another boot of the
        // same data-dir should not re-migrate anything (and not error).
        // In practice the second boot would see post-upgrade state and
        // never re-enter `run_legacy_upgrade`, but inside this function
        // a re-entry must observe State C and behave as a clean no-op.
        let meta = meta_pool().await;
        let auth = pre_stage3_auth_pool().await;

        run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();

        // After this point, profiles already has the slug-derived row.
        // Re-entering would crash on the UNIQUE(slug) constraint of
        // meta.profiles, which is the *expected* behaviour at this
        // function's level — `detect_boot_state` is the proper guard.
        // What we DO want to verify is that the data-move side is
        // idempotent in isolation:
        run_data_move(&meta, &auth)
            .await
            .expect("State C re-entry must be a clean no-op");

        // Meta.users count unchanged.
        assert_eq!(meta_users_count(&meta).await, 1);
    }

    /// The lock-row upsert is the load-bearing piece of the TOCTOU defence:
    /// it forces SQLite to acquire a RESERVED write lock at the start of
    /// the meta transaction, so the chunked email/id intersections that
    /// follow share a snapshot no concurrent writer can perturb. Verify
    /// it actually commits on State A success, and that it rolls back
    /// when the upgrade aborts (so the audit timestamp reflects only
    /// successful upgrade attempts).
    async fn lock_row_count(meta: &DatabaseConnection) -> i64 {
        meta.query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) FROM legacy_upgrade_locks",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index(0)
        .unwrap()
    }

    #[tokio::test]
    async fn state_a_success_commits_lock_row() {
        let meta = meta_pool().await;
        let auth = pre_stage3_auth_pool().await;

        assert_eq!(lock_row_count(&meta).await, 0, "lock table starts empty");

        run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();

        assert_eq!(
            lock_row_count(&meta).await,
            1,
            "successful State A must commit the lock-row upsert"
        );
    }

    #[tokio::test]
    async fn state_a_collision_abort_rolls_back_lock_row() {
        let meta = meta_pool().await;
        let auth = pre_stage3_auth_pool().await;

        // Seed a foreign user at id=1 to force user-id collision in State A.
        meta.execute_unprepared(
            "INSERT INTO users (id, email, name, password_hash, role_id, is_active,
                                created_at, updated_at)
             VALUES (1, 'someone-else@example.com', 'Someone Else', 'hash-2', 1, 1,
                     '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
        )
        .await
        .unwrap();

        let _err = run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap_err();

        assert_eq!(
            lock_row_count(&meta).await,
            0,
            "aborted upgrade must roll back the lock-row upsert so the audit \
             timestamp reflects only successful runs"
        );
    }

    #[tokio::test]
    async fn state_b_crash_recovery_does_not_persist_lock_row() {
        // State B path explicitly rolls back the meta tx (no inserts to
        // commit), so the lock row should NOT persist from this run.
        let meta = meta_pool().await;
        let auth = pre_stage3_auth_pool().await;

        meta.execute_unprepared(
            "INSERT INTO users (id, email, name, password_hash, role_id, is_active,
                                created_at, updated_at)
             VALUES (1, 'admin@example.com', 'Admin', 'hash-1', 1, 1,
                     '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
        )
        .await
        .unwrap();

        run_legacy_upgrade(
            &meta,
            &auth,
            "Acme",
            Path::new("/data/production/mokumo.db"),
            "production",
        )
        .await
        .unwrap();

        assert_eq!(
            lock_row_count(&meta).await,
            0,
            "State B (crash recovery) rolls back the meta tx — no lock row \
             persisted from this run since no fresh insert work happened"
        );
    }
}
