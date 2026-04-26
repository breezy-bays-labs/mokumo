use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
    /// rename that followed.
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

/// API response DTO for an activity log entry.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ActivityEntryResponse {
    #[ts(type = "number")]
    pub id: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub action: String,
    pub actor_id: String,
    pub actor_type: String,
    pub payload: Option<serde_json::Value>,
    pub created_at: String,
}

/// Wire mapping for activity entries.
///
/// R13 locks byte-identical output. The `created_at` column is stored as
/// `strftime('%Y-%m-%dT%H:%M:%SZ', 'now')` — second precision, literal `Z`
/// suffix. `DateTime::to_rfc3339()` emits `+00:00` and would break the
/// contract; this helper formats with `%Y-%m-%dT%H:%M:%SZ` so the wire
/// value round-trips the stored shape.
pub fn to_response(e: ActivityEntry) -> ActivityEntryResponse {
    ActivityEntryResponse {
        id: e.id,
        entity_type: e.entity_type,
        entity_id: e.entity_id,
        action: e.action,
        actor_id: e.actor_id,
        actor_type: e.actor_type,
        payload: Some(e.payload),
        created_at: e.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_entry() -> ActivityEntry {
        ActivityEntry {
            id: 42,
            entity_type: "customer".to_string(),
            entity_id: "abc".to_string(),
            action: "created".to_string(),
            actor_id: "user-1".to_string(),
            actor_type: "user".to_string(),
            payload: serde_json::json!({"display_name": "Acme"}),
            created_at: Utc.with_ymd_and_hms(2025, 11, 2, 14, 30, 0).unwrap(),
        }
    }

    #[test]
    fn export_bindings() {
        ActivityEntryResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export TypeScript bindings");
    }

    #[test]
    fn to_response_formats_created_at_with_literal_z_suffix() {
        let resp = to_response(sample_entry());
        assert_eq!(resp.created_at, "2025-11-02T14:30:00Z");
    }

    #[test]
    fn to_response_preserves_action_string_verbatim() {
        let mut e = sample_entry();
        e.action = "legacy_action_name".to_string();
        assert_eq!(to_response(e).action, "legacy_action_name");
    }

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
