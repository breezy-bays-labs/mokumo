//! CLI reset-password command — direct database password update.
//!
//! Opens the profile's SQLite database directly (no daemon required),
//! hashes the new password with Argon2id, and updates the user row.

use std::path::Path;

use crate::CliError;

/// Reset a user's password by email, opening the database file directly.
///
/// Returns an error if the database cannot be opened, the SQL fails, or
/// no active (non-deleted) user matches `email`.
pub fn run(db_path: &Path, email: &str, new_password: &str) -> Result<(), CliError> {
    // READ_WRITE only — never create a new DB file on a typo or missing path.
    let conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e| {
                CliError::Other(format!(
                    "Cannot open database at {}: {e}",
                    db_path.display()
                ))
            })?;

    let hash = password_auth::generate_hash(new_password);

    let rows = conn
        .execute(
            "UPDATE users SET password_hash = ?1 WHERE email = ?2 AND deleted_at IS NULL",
            rusqlite::params![hash, email],
        )
        .map_err(|e| CliError::Other(format!("Failed to update password: {e}")))?;

    if rows == 0 {
        return Err(CliError::Other(format!(
            "No active user found with email '{email}'"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE users (
                id INTEGER PRIMARY KEY,
                email TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                deleted_at TEXT
            );
            INSERT INTO users (email, password_hash) VALUES ('admin@shop.local', 'old_hash');",
        )
        .unwrap();
        (dir, db_path)
    }

    #[test]
    fn resets_password_for_active_user() {
        let (_dir, db_path) = create_test_db();

        run(&db_path, "admin@shop.local", "new_password_123").unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let hash: String = conn
            .query_row(
                "SELECT password_hash FROM users WHERE email = 'admin@shop.local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_ne!(hash, "old_hash", "password hash should have changed");
    }

    #[test]
    fn returns_error_for_unknown_email() {
        let (_dir, db_path) = create_test_db();

        let err = run(&db_path, "nobody@shop.local", "password123").unwrap_err();
        assert!(
            err.to_string().contains("No active user found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn skips_soft_deleted_users() {
        let (_dir, db_path) = create_test_db();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE users SET deleted_at = '2026-01-01' WHERE email = 'admin@shop.local'",
            [],
        )
        .unwrap();

        let err = run(&db_path, "admin@shop.local", "password123").unwrap_err();
        assert!(err.to_string().contains("No active user found"));
    }

    #[test]
    fn returns_error_for_missing_db() {
        let err = run(
            Path::new("/nonexistent/path/test.db"),
            "admin@shop.local",
            "password",
        )
        .unwrap_err();
        assert!(err.to_string().contains("Cannot open database"));
    }
}
