use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
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
}
