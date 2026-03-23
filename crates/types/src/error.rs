use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Wire format for API error responses.
///
/// Every non-2xx response from the API returns this shape.
/// `details` carries per-field validation messages when present.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, Vec<String>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        ErrorBody::export_all().expect("Failed to export ErrorBody TypeScript bindings");
    }

    #[test]
    fn serde_roundtrip_without_details() {
        let body = ErrorBody {
            code: "not_found".into(),
            message: "Customer not found".into(),
            details: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        let restored: ErrorBody = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.code, "not_found");
        assert_eq!(restored.message, "Customer not found");
        assert!(restored.details.is_none());
    }

    #[test]
    fn serde_roundtrip_with_details() {
        let mut details = HashMap::new();
        details.insert(
            "email".into(),
            vec!["invalid format".into(), "required".into()],
        );
        details.insert("name".into(), vec!["too short".into()]);

        let body = ErrorBody {
            code: "validation_error".into(),
            message: "Validation failed".into(),
            details: Some(details),
        };
        let json = serde_json::to_string(&body).unwrap();
        let restored: ErrorBody = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.code, "validation_error");
        let d = restored.details.unwrap();
        assert_eq!(d["email"].len(), 2);
        assert_eq!(d["name"], vec!["too short"]);
    }

    #[test]
    fn details_omitted_from_json_when_none() {
        let body = ErrorBody {
            code: "not_found".into(),
            message: "Not found".into(),
            details: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(
            !json.contains("details"),
            "details should be omitted when None, got: {json}"
        );
    }

    mod proptest_roundtrips {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn error_body_serialization_roundtrip(
                code in "[a-z_]{3,20}",
                message in "[a-zA-Z ]{1,50}",
            ) {
                let original = ErrorBody {
                    code,
                    message,
                    details: None,
                };
                let json = serde_json::to_string(&original).unwrap();
                let restored: ErrorBody = serde_json::from_str(&json).unwrap();
                assert_eq!(original.code, restored.code);
                assert_eq!(original.message, restored.message);
            }
        }
    }
}
