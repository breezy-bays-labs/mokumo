//! `PRAGMA application_id` used to identify a kikan-managed database.
//!
//! The stored byte value (`0x4D4B4D4F` — ASCII `"MKMO"` as a big-endian
//! 32-bit integer) is **frozen**: existing profile databases stamp this
//! header during the `m20260404_000000_set_pragmas` migration. The constant
//! was renamed from `MOKUMO_APPLICATION_ID` during Stage 3's platform lift
//! (#507) but the byte value must not change — downgrade/rename would trip
//! the [`check_application_id`] guard on any previously-created database.

use crate::db::DatabaseSetupError;

/// PRAGMA application_id value that identifies a kikan-managed database.
/// `"MKMO"` encoded as a big-endian 32-bit integer (0x4D4B4D4F = 1296780623).
///
/// Valid states at startup: `0` (not-yet-stamped, legacy installs before
/// `m20260404_000000_set_pragmas` ran) or this value. Any other non-zero
/// value → [`check_application_id`] returns
/// [`DatabaseSetupError::NotKikanDatabase`].
///
/// **Invariant**: the stored byte value is frozen at `0x4D4B4D4F` — existing
/// profile databases in the field stamp this value, and the identity guard
/// rejects any other non-zero value. The Rust symbol may be renamed freely;
/// the header value may not.
pub const KIKAN_APPLICATION_ID: i64 = 0x4D4B4D4F;

/// Check whether the database file belongs to kikan by reading PRAGMA
/// application_id.
///
/// Valid states:
/// - `0` — not yet stamped (existing installs before
///   `m20260404_000000_set_pragmas` runs); valid.
/// - [`KIKAN_APPLICATION_ID`] (1296780623, "MKMO") — stamped correctly;
///   valid.
/// - any other non-zero — not a kikan database; returns
///   [`DatabaseSetupError::NotKikanDatabase`].
///
/// Uses a raw `rusqlite::Connection` (pre-pool) so pool resources are never
/// allocated against an incompatible file.
///
/// # Important
/// Call this BEFORE opening any SQLx pool to the same database.
pub fn check_application_id(db_path: &std::path::Path) -> Result<(), DatabaseSetupError> {
    let conn = rusqlite::Connection::open(db_path)?;
    let app_id: i64 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    drop(conn);

    match app_id {
        0 => Ok(()),                                // not-yet-stamped — valid
        id if id == KIKAN_APPLICATION_ID => Ok(()), // "MKMO" — valid
        _ => Err(DatabaseSetupError::NotKikanDatabase {
            path: db_path.to_path_buf(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kikan_application_id_byte_value_is_frozen() {
        // Continuity guarantee: existing profile DBs stamped with this value
        // will trip `check_application_id` if it drifts.
        assert_eq!(KIKAN_APPLICATION_ID, 0x4D4B4D4F);
        assert_eq!(KIKAN_APPLICATION_ID, 1296780623);
    }
}
