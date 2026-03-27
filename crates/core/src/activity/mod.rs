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
}

impl std::fmt::Display for ActivityAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Updated => write!(f, "updated"),
            Self::SoftDeleted => write!(f, "soft_deleted"),
            Self::Restored => write!(f, "restored"),
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
    }
}
