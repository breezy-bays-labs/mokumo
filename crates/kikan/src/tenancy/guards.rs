use std::path::Path;

use crate::error::TenancyError;

pub const MOKUMO_APPLICATION_ID: i64 = 0x4D4B4D4F;

pub fn check_application_id(db_path: &Path) -> Result<(), TenancyError> {
    let conn = rusqlite::Connection::open(db_path)?;
    let app_id: i64 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    drop(conn);

    match app_id {
        0 => Ok(()),
        id if id == MOKUMO_APPLICATION_ID => Ok(()),
        _ => Err(TenancyError::NotMokumoDatabase {
            path: db_path.to_path_buf(),
        }),
    }
}

pub fn check_schema_compatibility(
    db_path: &Path,
    known_migrations: &[crate::MigrationRef],
) -> Result<(), TenancyError> {
    if !db_path.exists() {
        return Ok(());
    }

    let conn = rusqlite::Connection::open(db_path)?;

    let has_kikan = table_exists(&conn, "kikan_migrations")?;
    let has_seaql = !has_kikan && table_exists(&conn, "seaql_migrations")?;

    let unknown = if has_kikan {
        check_kikan_migrations(&conn, known_migrations)?
    } else if has_seaql {
        check_seaql_migrations(&conn, known_migrations)?
    } else {
        return Ok(());
    };
    drop(conn);

    if unknown.is_empty() {
        Ok(())
    } else {
        Err(TenancyError::SchemaIncompatible {
            path: db_path.to_path_buf(),
            unknown_migrations: unknown,
        })
    }
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |row| row.get(0),
    )
}

fn check_kikan_migrations(
    conn: &rusqlite::Connection,
    known: &[crate::MigrationRef],
) -> Result<Vec<String>, rusqlite::Error> {
    let applied: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT graft_id, name FROM kikan_migrations")?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let known_set: std::collections::HashSet<(&str, &str)> =
        known.iter().map(|r| (r.graft.get(), r.name)).collect();
    Ok(applied
        .into_iter()
        .filter(|(g, n)| !known_set.contains(&(g.as_str(), n.as_str())))
        .map(|(g, n)| format!("{g}::{n}"))
        .collect())
}

fn check_seaql_migrations(
    conn: &rusqlite::Connection,
    known: &[crate::MigrationRef],
) -> Result<Vec<String>, rusqlite::Error> {
    let applied: Vec<String> = {
        let mut stmt = conn.prepare("SELECT version FROM seaql_migrations")?;
        stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let known_names: std::collections::HashSet<&str> = known.iter().map(|r| r.name).collect();
    Ok(applied
        .into_iter()
        .filter(|v| !known_names.contains(v.as_str()))
        .collect())
}

pub fn ensure_auto_vacuum(db_path: &Path) -> Result<(), TenancyError> {
    if !db_path.exists() {
        let conn = rusqlite::Connection::open(db_path)?;
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL")?;
        tracing::info!(
            "Created new database with auto_vacuum=INCREMENTAL at {}",
            db_path.display()
        );
        drop(conn);
        return Ok(());
    }

    let conn = rusqlite::Connection::open(db_path)?;
    let current: i32 = conn.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;

    match current {
        0 => {
            tracing::info!(
                "Upgrading auto_vacuum from NONE to INCREMENTAL on {}; running one-time VACUUM",
                db_path.display()
            );
            conn.execute_batch("PRAGMA auto_vacuum = 2; VACUUM;")?;
        }
        1 | 2 => {}
        other => {
            tracing::warn!(
                "Unexpected auto_vacuum value {other} on {}; skipping upgrade",
                db_path.display()
            );
        }
    }

    drop(conn);
    Ok(())
}

pub async fn pre_migration_backup(
    db_path: &Path,
) -> Result<Option<std::path::PathBuf>, TenancyError> {
    match tokio::fs::metadata(db_path).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(e) => {
            return Err(TenancyError::Backup(format!(
                "cannot stat {}: {e}",
                db_path.display()
            )));
        }
    }

    let version = {
        let path = db_path.to_path_buf();
        tokio::task::spawn_blocking(move || -> Result<Option<String>, TenancyError> {
            let conn = rusqlite::Connection::open(&path)?;
            let table_exists: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name IN ('kikan_migrations', 'seaql_migrations')",
                [],
                |row| row.get(0),
            )?;
            if !table_exists {
                return Ok(None);
            }
            let kikan_exists: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='kikan_migrations'",
                [],
                |row| row.get(0),
            )?;
            let v: String = if kikan_exists {
                conn.query_row(
                    "SELECT COALESCE(MAX(name), 'unknown') FROM kikan_migrations",
                    [],
                    |row| row.get(0),
                )?
            } else {
                conn.query_row(
                    "SELECT COALESCE(MAX(version), 'unknown') FROM seaql_migrations",
                    [],
                    |row| row.get(0),
                )?
            };
            Ok(Some(v))
        })
        .await
        .map_err(|e| TenancyError::Backup(format!("spawn_blocking join: {e}")))??
    };

    let version = match version {
        Some(v) => v,
        None => return Ok(None),
    };

    let file_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| TenancyError::Backup("invalid database path".to_string()))?;
    let backup_path = db_path.with_file_name(format!("{file_name}.backup-v{version}"));

    let backup_clone = backup_path.clone();
    let source = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), TenancyError> {
        let src = rusqlite::Connection::open(&source)?;
        let mut dst = rusqlite::Connection::open(&backup_clone)?;
        let backup = rusqlite::backup::Backup::new(&src, &mut dst)?;
        backup.run_to_completion(5, std::time::Duration::from_millis(250), None)?;
        Ok(())
    })
    .await
    .map_err(|e| TenancyError::Backup(format!("backup join: {e}")))??;

    tracing::info!("Created database backup at {:?}", backup_path);

    if let Err(e) = rotate_backups(db_path, 3).await {
        tracing::warn!("Backup rotation failed: {e}");
    }

    Ok(Some(backup_path))
}

async fn rotate_backups(db_path: &Path, keep: usize) -> Result<(), std::io::Error> {
    let parent = db_path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory")
    })?;
    let file_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid filename"))?;
    let prefix = format!("{file_name}.backup-v");

    let mut backups = Vec::new();
    let mut entries = tokio::fs::read_dir(parent).await?;
    while let Some(entry) = entries.next_entry().await? {
        if let Some(name) = entry.file_name().to_str()
            && name.starts_with(&prefix)
        {
            backups.push(entry.path());
        }
    }
    backups.sort();

    let to_delete = backups.len().saturating_sub(keep);
    for path in backups.into_iter().take(to_delete) {
        if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::warn!("Failed to remove old backup {:?}: {e}", path);
        }
    }

    Ok(())
}
