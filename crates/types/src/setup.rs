use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupStatusResponse {
    pub setup_complete: bool,
    #[ts(type = "\"demo\" | \"production\" | null")]
    pub setup_mode: Option<mokumo_core::setup::SetupMode>,
    pub is_first_launch: bool,
    pub production_setup_complete: bool,
    pub shop_name: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DemoResetResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct ProfileSwitchRequest {
    #[ts(type = "\"demo\" | \"production\"")]
    pub profile: mokumo_core::setup::SetupMode,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProfileSwitchResponse {
    #[ts(type = "\"demo\" | \"production\"")]
    pub profile: mokumo_core::setup::SetupMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RestoreValidateResponse {
    pub file_name: String,
    #[ts(type = "number")]
    pub file_size: u64,
    pub schema_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RestoreResponse {
    pub success: bool,
    pub message: String,
}
