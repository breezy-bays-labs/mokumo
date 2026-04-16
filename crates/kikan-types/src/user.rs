use serde::Serialize;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        UserResponse::export_all(&ts_rs::Config::from_env())
            .expect("Failed to export UserResponse");
    }
}
