use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UserResponse {
    #[ts(type = "number")]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub role_name: String,
    pub is_active: bool,
    pub last_login_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct UpdateUserRoleRequest {
    #[ts(type = "number")]
    pub role_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        let config = ts_rs::Config::from_env();
        UserResponse::export_all(&config).expect("Failed to export UserResponse");
        UpdateUserRoleRequest::export_all(&config).expect("Failed to export UpdateUserRoleRequest");
    }
}
