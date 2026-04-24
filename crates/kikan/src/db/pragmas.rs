//! PRAGMA configuration applied to every SQLite connection pool in the
//! kikan platform.
//!
//! # PRAGMAs set
//!
//! - `journal_mode=WAL` + `busy_timeout=5000` form the **in-process
//!   concurrent-safety** pair. WAL lets readers proceed while a writer
//!   holds the write lock; `busy_timeout` gives competing writers a
//!   5-second retry window before `SQLITE_BUSY` surfaces. See the
//!   crate-root docs for where this fits in the full startup-safety
//!   contract.
//! - `synchronous=NORMAL` — safe under WAL (fsync on checkpoint, not
//!   every commit).
//! - `foreign_keys=ON` — enforced per connection; SQLite requires this
//!   pragma on every handle, not just once per database.
//! - `cache_size=-64000` — 64000 KiB per connection (negative value =
//!   KiB; ≈62.5 MiB).
//! - `mmap_size` — set to [`CONFIGURED_MMAP_SIZE`] (non-zero on Linux
//!   only; see its docs for platform rationale).
//!
//! # What this does NOT cover
//!
//! Cross-process exclusion against the same data directory is the
//! caller's responsibility. WAL + `busy_timeout` make a single pool
//! safe for concurrent in-process work; they do not coordinate
//! operations between two Engines on the same directory. Sidecar
//! swaps (demo reset, restore) manipulate the database files via
//! paths kikan controls, outside SQLite's locking protocol, so two
//! Engines racing a swap corrupt each other. Backup destination
//! filenames are app-chosen: concurrent backups race the filesystem,
//! not SQLite's locks. Migration runs serialize through SQLite's
//! write lock, but the losing Engine fails to boot (spurious
//! migration errors or a `SQLITE_BUSY` once `busy_timeout` elapses)
//! rather than cooperating. Single-Engine enforcement is an
//! Application-level concern.

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
