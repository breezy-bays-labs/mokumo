//! PRAGMA configuration applied to every SQLite connection pool in the
//! kikan platform.
//!
//! WAL mode, normal synchronous, 5s busy timeout, foreign keys enforced,
//! 64MB cache. `mmap_size` is set to [`CONFIGURED_MMAP_SIZE`] — non-zero on
//! Linux only (see docs below).

use std::future::Future;
use std::pin::Pin;

use sqlx::sqlite::SqliteConnection;

/// Effective `mmap_size` for the SQLite connection pool, selected at compile time by
/// target platform.
///
/// - **Linux**: 256 MB — mmap delivers clear read-throughput gains on Linux's page
///   cache model.
/// - **Windows**: 0 (disabled) — the Windows kernel cannot truncate memory-mapped
///   files, so enabling mmap causes `incremental_vacuum` to silently fail to shrink
///   the database file.
/// - **macOS**: 0 (disabled) — the macOS unified buffer cache already provides the
///   I/O coalescing that mmap would add, so the benefit is negligible per the SQLite
///   developers. Disabling keeps behavior consistent with Windows and avoids historic
///   macOS mmap edge cases.
pub const CONFIGURED_MMAP_SIZE: i64 = if cfg!(target_os = "linux") {
    256 * 1024 * 1024
} else {
    0
};

/// Standard PRAGMAs applied to every SQLite connection pool managed by
/// kikan.
pub(crate) fn configure_sqlite_connection(
    conn: &mut SqliteConnection,
) -> Pin<Box<dyn Future<Output = Result<(), sqlx::Error>> + Send + '_>> {
    Box::pin(async move {
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&mut *conn)
            .await?;
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&mut *conn)
            .await?;
        sqlx::query("PRAGMA busy_timeout=5000")
            .execute(&mut *conn)
            .await?;
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&mut *conn)
            .await?;
        sqlx::query("PRAGMA cache_size=-64000")
            .execute(&mut *conn)
            .await?;
        sqlx::query(&format!("PRAGMA mmap_size={CONFIGURED_MMAP_SIZE}"))
            .execute(&mut *conn)
            .await?;
        Ok(())
    })
}
