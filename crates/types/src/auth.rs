use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::user::UserResponse;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ResetPasswordRequest {
    pub email: String,
    pub pin: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RecoverRequest {
    pub email: String,
    pub recovery_code: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupRequest {
    pub shop_name: String,
    pub admin_name: String,
    pub admin_email: String,
    pub admin_password: String,
    pub setup_token: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SetupResponse {
    pub recovery_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RegenerateRecoveryCodesRequest {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct MeResponse {
    pub user: UserResponse,
    pub setup_complete: bool,
    pub recovery_codes_remaining: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        LoginRequest::export_all().expect("Failed to export LoginRequest");
        SetupRequest::export_all().expect("Failed to export SetupRequest");
        SetupResponse::export_all().expect("Failed to export SetupResponse");
        MeResponse::export_all().expect("Failed to export MeResponse");
        ForgotPasswordRequest::export_all().expect("Failed to export ForgotPasswordRequest");
        ResetPasswordRequest::export_all().expect("Failed to export ResetPasswordRequest");
        RecoverRequest::export_all().expect("Failed to export RecoverRequest");
        RegenerateRecoveryCodesRequest::export_all()
            .expect("Failed to export RegenerateRecoveryCodesRequest");
    }
}
