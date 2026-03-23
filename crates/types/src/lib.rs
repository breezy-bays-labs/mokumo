pub mod error;
pub mod ws;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        HealthResponse::export_all().expect("Failed to export TypeScript bindings");
    }

    mod proptest_roundtrips {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn health_response_serialization_roundtrip(
                status in "[a-zA-Z_]{1,20}",
                version in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}",
            ) {
                let original = HealthResponse { status, version };
                let json = serde_json::to_string(&original).unwrap();
                let restored: HealthResponse = serde_json::from_str(&json).unwrap();
                assert_eq!(original, restored);
            }
        }
    }
}
