use mokumo_core::activity::ActivityEntry;
use serde::Serialize;
use ts_rs::TS;

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
    use chrono::{TimeZone, Utc};

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
}
