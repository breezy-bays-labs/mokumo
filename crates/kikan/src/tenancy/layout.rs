use std::path::Path;

use super::SetupMode;
use crate::error::TenancyError;

pub fn migrate_flat_layout(data_dir: &Path) -> Result<(), TenancyError> {
    let flat_db = data_dir.join("mokumo.db");
    let production_dir = data_dir.join(SetupMode::Production.as_dir_name());
    let production_db = production_dir.join("mokumo.db");
    let profile_path = data_dir.join("active_profile");

    let flat_exists = flat_db.try_exists()?;
    if !flat_exists {
        return Ok(());
    }

    copy_flat_to_production(&flat_db, &production_db, &production_dir)?;
    write_active_profile(&profile_path, flat_exists)?;
    remove_flat_files(data_dir, &flat_db)?;

    Ok(())
}

fn copy_flat_to_production(
    flat_db: &Path,
    production_db: &Path,
    production_dir: &Path,
) -> Result<(), TenancyError> {
    if production_db.try_exists()? {
        return Ok(());
    }
    std::fs::create_dir_all(production_dir)?;
    if let Ok(conn) = rusqlite::Connection::open(flat_db)
        && let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
    {
        tracing::warn!(
            "WAL checkpoint failed during flat DB migration (proceeding with copy): {e}"
        );
    }
    std::fs::copy(flat_db, production_db)?;
    tracing::info!("Migrated flat database to {}", production_db.display());
    Ok(())
}

fn write_active_profile(profile_path: &Path, flat_exists: bool) -> Result<(), TenancyError> {
    if !profile_path.try_exists()? && flat_exists {
        std::fs::write(profile_path, SetupMode::Production.as_str())?;
        tracing::info!("Set active profile to 'production' (migrated from flat layout)");
    }
    Ok(())
}

fn remove_flat_files(data_dir: &Path, flat_db: &Path) -> Result<(), TenancyError> {
    std::fs::remove_file(flat_db)?;
    let _ = std::fs::remove_file(data_dir.join("mokumo.db-wal"));
    let _ = std::fs::remove_file(data_dir.join("mokumo.db-shm"));
    Ok(())
}
