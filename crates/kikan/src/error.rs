use crate::migrations::{GraftId, MigrationRef};

#[derive(Debug, thiserror::Error)]
pub enum DagError {
    #[error("cycle detected: {}", format_path(path))]
    Cycle { path: Vec<MigrationRef> },

    #[error("dangling dependency: {from} depends on {to} which does not exist")]
    DanglingRef {
        from: MigrationRef,
        to: MigrationRef,
    },

    #[error("duplicate migration {name} in graft {graft}")]
    DuplicateMigration { graft: GraftId, name: &'static str },

    #[error(
        "cross-target dependency: Meta migration {meta} cannot depend on PerProfile migration {per_profile}"
    )]
    CrossTargetViolation {
        meta: MigrationRef,
        per_profile: MigrationRef,
    },
}

fn format_path(path: &[MigrationRef]) -> String {
    path.iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[derive(Debug, thiserror::Error)]
#[error("migration {graft}::{name} failed: {source}")]
pub struct MigrationError {
    pub graft: GraftId,
    pub name: &'static str,
    pub source: sea_orm::DbErr,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("boot error: {0}")]
    Boot(String),

    /// Refused to boot because the legacy `production/` data directory has
    /// admin user(s) but a blank `shop_settings.shop_name` — auto-deriving
    /// a slug from an empty name would produce an empty profile directory.
    /// Operator must repair the legacy DB manually before re-launching.
    #[error(
        "legacy install refuses to boot: shop_name is empty at {}; manual repair required before re-launch",
        path.display()
    )]
    DefensiveEmptyShopName { path: std::path::PathBuf },

    #[error("boot-state detection failed: {0}")]
    BootStateDetection(#[from] crate::meta::BootStateDetectionError),

    #[error("legacy upgrade failed: {0}")]
    LegacyUpgrade(#[from] crate::meta::UpgradeError),

    #[error(transparent)]
    Migration(#[from] MigrationError),

    #[error(transparent)]
    Dag(#[from] DagError),

    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),

    #[error("serve error: {0}")]
    Serve(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ActivityWriteError {
    #[error("failed to serialize activity payload: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
}

#[derive(Debug, thiserror::Error)]
pub enum TenancyError {
    #[error("profile not found: {profile}")]
    ProfileNotFound { profile: String },

    #[error("not a Mokumo database: {}", path.display())]
    NotMokumoDatabase { path: std::path::PathBuf },

    #[error("schema incompatible: database at {} has unknown migrations: {:?}", path.display(), unknown_migrations)]
    SchemaIncompatible {
        path: std::path::PathBuf,
        unknown_migrations: Vec<String>,
    },

    #[error("backup error: {0}")]
    Backup(String),

    #[error("layout migration error: {0}")]
    Layout(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),

    #[error("rusqlite error: {0}")]
    Rusqlite(#[from] rusqlite::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum AppHandleError {
    #[error("{0}")]
    NotAvailable(String),
}

/// Domain-level errors representing business rule violations.
///
/// These are framework-agnostic — no HTTP, no Axum. The API layer
/// converts them into AppError with appropriate status codes.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("{entity} with id {id} not found")]
    NotFound { entity: &'static str, id: String },

    #[error("{message}")]
    Conflict { message: String },

    #[error("Validation failed")]
    Validation {
        details: std::collections::HashMap<String, Vec<String>>,
    },

    #[error("{message}")]
    Internal { message: String },
}

#[cfg(test)]
mod domain_error_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn not_found_display_includes_entity_and_id() {
        let err = DomainError::NotFound {
            entity: "widget",
            id: "42".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("widget") && msg.contains("42"),
            "Expected display to contain entity and id, got: {msg}"
        );
    }

    #[test]
    fn conflict_display_includes_message() {
        let err = DomainError::Conflict {
            message: "email already exists".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("email already exists"),
            "Expected display to contain message, got: {msg}"
        );
    }

    #[test]
    fn validation_display_is_descriptive() {
        let mut details = HashMap::new();
        details.insert("email".into(), vec!["invalid format".into()]);
        let err = DomainError::Validation { details };
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("validation"),
            "Expected display to mention validation, got: {msg}"
        );
    }

    #[test]
    fn internal_display_includes_message() {
        let err = DomainError::Internal {
            message: "disk full".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("disk full"),
            "Expected display to contain message, got: {msg}"
        );
    }
}
