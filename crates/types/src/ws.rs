use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BroadcastEvent {
    pub v: u8,
    #[serde(rename = "type")]
    pub type_: String,
    pub topic: String,
    pub payload: serde_json::Value,
}

impl BroadcastEvent {
    /// Create a new BroadcastEvent with v=1 and topic derived from the event type.
    ///
    /// The topic is the prefix before the first `.` in the type string
    /// (e.g. `"customer.created"` → topic `"customer"`).
    pub fn new(type_: impl Into<String>, payload: serde_json::Value) -> Self {
        let type_ = type_.into();
        let topic = type_.split('.').next().unwrap_or("unknown").to_string();
        Self {
            v: 1,
            type_,
            topic,
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let event = BroadcastEvent {
            v: 1,
            type_: "customer.created".into(),
            topic: "customer".into(),
            payload: serde_json::json!({"id": 42, "name": "Test Shop"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        let restored: BroadcastEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, restored);
    }

    #[test]
    fn serde_field_rename() {
        let event = BroadcastEvent {
            v: 1,
            type_: "order.updated".into(),
            topic: "order".into(),
            payload: serde_json::Value::Null,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"order.updated""#));
        assert!(!json.contains("type_"));
    }

    #[test]
    fn deserialize_from_json_string() {
        let json = r#"{"v":1,"type":"job.completed","topic":"job","payload":{"id":1}}"#;
        let event: BroadcastEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.v, 1);
        assert_eq!(event.type_, "job.completed");
        assert_eq!(event.topic, "job");
        assert_eq!(event.payload["id"], 1);
    }

    #[test]
    fn new_derives_topic_from_type() {
        let event = BroadcastEvent::new("customer.created", serde_json::json!({"id": 1}));
        assert_eq!(event.v, 1);
        assert_eq!(event.type_, "customer.created");
        assert_eq!(event.topic, "customer");
        assert_eq!(event.payload["id"], 1);
    }

    #[test]
    fn new_handles_no_dot_in_type() {
        let event = BroadcastEvent::new("ping", serde_json::Value::Null);
        assert_eq!(event.topic, "ping");
    }

    #[test]
    fn export_bindings() {
        BroadcastEvent::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export TypeScript bindings");
    }
}
