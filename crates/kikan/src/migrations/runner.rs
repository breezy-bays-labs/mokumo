//! Per-migration transaction runner.
//!
//! Each migration runs inside its own `SqliteTransactionMode::Immediate`
//! transaction — the write lock is acquired on `BEGIN IMMEDIATE` rather
//! than lazily on first write. That matters for concurrent boot safety:
//! a migration cannot be mid-flight on a deferred transaction, get
//! overtaken by a concurrent write from application code, and then eat
//! `SQLITE_BUSY` when it tries to upgrade to write. Either the
//! migration holds the write lock for its whole duration or it fails
//! cleanly on acquire.
//!
//! Tracking lives in the `kikan_migrations` table; bootstrap of that
//! table runs once before any migration body. See
//! `tests/features/migration_execution.feature` for the behavioral
//! contract.

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement, Value,
};
use std::sync::Arc;
use tracing::info;

use crate::error::{EngineError, MigrationError};
use crate::migrations::GraftId;
use crate::migrations::Migration;
use crate::migrations::bootstrap;
use crate::migrations::conn::MigrationConn;
use crate::migrations::dag;

pub async fn run_migrations(
    pool: &DatabaseConnection,
    all_migrations: &[Arc<dyn Migration>],
) -> Result<(), EngineError> {
    run_migrations_with_backfill(pool, all_migrations, None).await
}

pub async fn run_migrations_with_backfill(
    pool: &DatabaseConnection,
    all_migrations: &[Arc<dyn Migration>],
    backfill_graft_id: Option<GraftId>,
) -> Result<(), EngineError> {
    bootstrap_tables(pool).await?;

    if let Some(graft_id) = backfill_graft_id {
        backfill_seaql_if_present(pool, graft_id).await?;
    }

    let applied = query_applied(pool).await?;
    let ordered = dag::resolve(all_migrations)?;

    let unapplied: Vec<_> = ordered
        .into_iter()
        .filter(|m| !applied.contains(&(m.graft_id().get().to_string(), m.name().to_string())))
        .collect();

    if unapplied.is_empty() {
        info!("all migrations already applied");
        return Ok(());
    }

    info!(count = unapplied.len(), "applying migrations");

    pool.execute_unprepared("PRAGMA foreign_keys = OFF").await?;

    let batch_result = async {
        for migration in &unapplied {
            let graft = migration.graft_id();
            let name = migration.name();
            info!(%graft, name, "applying migration");

            apply_single(pool, migration.as_ref()).await?;

            info!(%graft, name, "migration applied");
        }
        Ok::<(), EngineError>(())
    }
    .await;

    // `PRAGMA foreign_keys` is per-connection. The OFF call above acquires
    // a fresh pool connection from `pool.execute_unprepared`, leaving FK
    // disabled on whichever pool member answered. A naive `pool.execute_
    // unprepared("PRAGMA foreign_keys = ON")` restore can land on a
    // *different* member, so the OFF-stuck connection would silently
    // violate the `after_connect` contract that FK is ON for the next
    // acquirer. Cycle every member: acquire `max_connections` times
    // (creating any not-yet-opened slot), set FK=ON, hold them all so we
    // don't re-acquire the same connection, then drop the whole batch
    // back into the pool with a uniform FK=ON state.
    let sqlite_pool = pool.get_sqlite_connection_pool();
    let max = sqlite_pool.options().get_max_connections() as usize;
    let mut held: Vec<sqlx::pool::PoolConnection<sqlx::Sqlite>> = Vec::with_capacity(max);
    for _ in 0..max {
        let mut c = sqlite_pool
            .acquire()
            .await
            .map_err(|e| sea_orm::DbErr::Custom(format!("acquire FK reset connection: {e}")))?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *c)
            .await
            .map_err(|e| sea_orm::DbErr::Custom(format!("PRAGMA foreign_keys = ON: {e}")))?;
        held.push(c);
    }
    drop(held);
    batch_result?;

    let fk_violations: Vec<sea_orm::JsonValue> = sea_orm::JsonValue::find_by_statement(
        Statement::from_string(DatabaseBackend::Sqlite, "PRAGMA foreign_key_check"),
    )
    .all(pool)
    .await?;

    if !fk_violations.is_empty() {
        tracing::warn!(
            count = fk_violations.len(),
            "foreign key violations found after migration batch"
        );
    }

    Ok(())
}

async fn apply_single(
    pool: &DatabaseConnection,
    migration: &dyn Migration,
) -> Result<(), EngineError> {
    use sea_orm::sea_query::{Alias, Expr, Query};
    use sea_orm::{SqliteTransactionMode, TransactionOptions, TransactionTrait};

    let graft = migration.graft_id();
    let name = migration.name();

    let txn = pool
        .begin_with_options(TransactionOptions {
            sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
            ..Default::default()
        })
        .await?;
    let conn = MigrationConn::new(txn);

    migration.up(&conn).await.map_err(|source| MigrationError {
        graft,
        name,
        source,
    })?;

    let inner = conn.into_inner();
    let insert = Query::insert()
        .into_table(Alias::new("kikan_migrations"))
        .columns([
            Alias::new("graft_id"),
            Alias::new("name"),
            Alias::new("applied_at"),
        ])
        .values_panic([
            Value::from(graft.get()).into(),
            Value::from(name).into(),
            Expr::cust("unixepoch()"),
        ])
        .to_owned();
    inner.execute(&insert).await?;
    inner.commit().await?;

    Ok(())
}

async fn bootstrap_tables(pool: &DatabaseConnection) -> Result<(), EngineError> {
    pool.execute_unprepared(bootstrap::KIKAN_MIGRATIONS_SQL)
        .await?;
    pool.execute_unprepared(bootstrap::KIKAN_META_SQL).await?;

    pool.execute_unprepared(
        "INSERT OR IGNORE INTO kikan_migrations (graft_id, name, applied_at) VALUES ('kikan', 'create_kikan_migrations', unixepoch())",
    )
    .await?;
    pool.execute_unprepared(
        "INSERT OR IGNORE INTO kikan_migrations (graft_id, name, applied_at) VALUES ('kikan', 'create_kikan_meta', unixepoch())",
    )
    .await?;

    Ok(())
}

pub async fn backfill_seaql_if_present(
    pool: &DatabaseConnection,
    graft_id: GraftId,
) -> Result<usize, EngineError> {
    #[derive(Debug, FromQueryResult)]
    struct TableCheck {
        cnt: i64,
    }

    let seaql_exists: Vec<TableCheck> = TableCheck::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT COUNT(*) as cnt FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
    ))
    .all(pool)
    .await?;

    if seaql_exists.is_empty() || seaql_exists[0].cnt == 0 {
        return Ok(0);
    }

    #[derive(Debug, FromQueryResult)]
    struct SeaqlRow {
        version: String,
        applied_at: i64,
    }

    let rows: Vec<SeaqlRow> = SeaqlRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT version, applied_at FROM seaql_migrations",
    ))
    .all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    for row in &rows {
        use sea_orm::sea_query::{Alias, Expr, OnConflict, Query, SqliteQueryBuilder};

        let insert = Query::insert()
            .into_table(Alias::new("kikan_migrations"))
            .columns([
                Alias::new("graft_id"),
                Alias::new("name"),
                Alias::new("applied_at"),
            ])
            .values_panic([
                Value::from(graft_id.get()).into(),
                Value::from(row.version.as_str()).into(),
                Expr::val(row.applied_at),
            ])
            .on_conflict(
                OnConflict::columns([Alias::new("graft_id"), Alias::new("name")])
                    .do_nothing()
                    .to_owned(),
            )
            .to_owned();

        let sql = insert.to_string(SqliteQueryBuilder);
        pool.execute_unprepared(&sql).await?;
        count += 1;
    }

    info!(count, graft = %graft_id, "backfilled seaql_migrations into kikan_migrations");
    Ok(count)
}

async fn query_applied(pool: &DatabaseConnection) -> Result<Vec<(String, String)>, EngineError> {
    #[derive(Debug, FromQueryResult)]
    struct AppliedRow {
        graft_id: String,
        name: String,
    }

    let rows: Vec<AppliedRow> = AppliedRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT graft_id, name FROM kikan_migrations",
    ))
    .all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| (r.graft_id, r.name)).collect())
}
