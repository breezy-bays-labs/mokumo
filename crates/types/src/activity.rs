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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        ActivityEntryResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export TypeScript bindings");
    }
}
