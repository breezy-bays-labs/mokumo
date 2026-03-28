use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupStatusResponse {
    pub setup_complete: bool,
    #[ts(type = "\"demo\" | \"production\" | null")]
    pub setup_mode: Option<mokumo_core::setup::SetupMode>,
}
