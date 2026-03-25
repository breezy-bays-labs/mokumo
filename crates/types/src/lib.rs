pub mod activity;
pub mod customer;
pub mod error;
pub mod pagination;
pub mod ws;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    #[ts(type = "number")]
    pub uptime_seconds: u64,
    pub database: String,
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
                uptime_seconds in 0u64..1_000_000,
                database in "[a-zA-Z_]{1,10}",
            ) {
                let original = HealthResponse { status, version, uptime_seconds, database };
                let json = serde_json::to_string(&original).unwrap();
                let restored: HealthResponse = serde_json::from_str(&json).unwrap();
                assert_eq!(original, restored);
            }
        }
    }
}
