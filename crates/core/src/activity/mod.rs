pub mod traits;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Actions that can be recorded in the activity log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityAction {
    Created,
    Updated,
    SoftDeleted,
    Restored,
    LoginSuccess,
    LoginFailed,
    PasswordChanged,
    SetupCompleted,
    PasswordReset,
    RecoveryCodesRegenerated,
    RoleUpdated,
    /// User account locked after too many failed login attempts.
    AccountLocked,
    /// User account unlocked by an admin.
    AccountUnlocked,
    /// First-admin bootstrap on an empty user table.
    Bootstrap,
    /// A pre-meta-DB install with a completed shop_settings row was migrated
    /// into `meta.profiles` at boot time. Written to `meta.activity_log`.
    /// Payload carries the original `shop_name` and the legacy vertical DB
    /// path so an operator can correlate the audit entry with the on-disk
    /// rename that followed (`production/` → `<derived-slug>/`).
    LegacyUpgradeMigrated,
}

impl std::fmt::Display for ActivityAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Updated => write!(f, "updated"),
            Self::SoftDeleted => write!(f, "soft_deleted"),
            Self::Restored => write!(f, "restored"),
            Self::LoginSuccess => write!(f, "login_success"),
            Self::LoginFailed => write!(f, "login_failed"),
            Self::PasswordChanged => write!(f, "password_changed"),
            Self::SetupCompleted => write!(f, "setup_completed"),
            Self::PasswordReset => write!(f, "password_reset"),
            Self::RecoveryCodesRegenerated => write!(f, "recovery_codes_regenerated"),
            Self::RoleUpdated => write!(f, "role_updated"),
            Self::AccountLocked => write!(f, "account_locked"),
            Self::AccountUnlocked => write!(f, "account_unlocked"),
            Self::Bootstrap => write!(f, "bootstrap"),
            Self::LegacyUpgradeMigrated => write!(f, "legacy_upgrade_migrated"),
        }
    }
}

/// Domain entity representing a single activity log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub action: String,
    pub actor_id: String,
    pub actor_type: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_action_serializes_snake_case() {
        let json = serde_json::to_string(&ActivityAction::SoftDeleted).unwrap();
        assert_eq!(json, r#""soft_deleted""#);
    }

    #[test]
    fn activity_action_display() {
        assert_eq!(ActivityAction::Created.to_string(), "created");
        assert_eq!(ActivityAction::Updated.to_string(), "updated");
        assert_eq!(ActivityAction::SoftDeleted.to_string(), "soft_deleted");
        assert_eq!(ActivityAction::Restored.to_string(), "restored");
        assert_eq!(ActivityAction::LoginSuccess.to_string(), "login_success");
        assert_eq!(ActivityAction::LoginFailed.to_string(), "login_failed");
        assert_eq!(
            ActivityAction::PasswordChanged.to_string(),
            "password_changed"
        );
        assert_eq!(
            ActivityAction::SetupCompleted.to_string(),
            "setup_completed"
        );
        assert_eq!(ActivityAction::PasswordReset.to_string(), "password_reset");
        assert_eq!(ActivityAction::RoleUpdated.to_string(), "role_updated");
        assert_eq!(
            ActivityAction::RecoveryCodesRegenerated.to_string(),
            "recovery_codes_regenerated"
        );
        assert_eq!(ActivityAction::AccountLocked.to_string(), "account_locked");
        assert_eq!(
            ActivityAction::AccountUnlocked.to_string(),
            "account_unlocked"
        );
        assert_eq!(ActivityAction::Bootstrap.to_string(), "bootstrap");
        assert_eq!(
            ActivityAction::LegacyUpgradeMigrated.to_string(),
            "legacy_upgrade_migrated"
        );
    }
}
