//! Platform auth domain types.

use crate::error::DomainError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(i64);

impl UserId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoleId(i64);

impl RoleId {
    pub const ADMIN: RoleId = RoleId(1);
    pub const STAFF: RoleId = RoleId(2);
    pub const GUEST: RoleId = RoleId(3);

    pub fn new(id: i64) -> Self {
        Self(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for RoleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub name: String,
    pub role_id: RoleId,
    pub is_active: bool,
    pub last_login_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: RoleId,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct CreateUser {
    pub email: String,
    pub name: String,
    pub password: String,
    pub role_id: RoleId,
}

/// Port for user persistence operations.
pub trait UserRepository: Send + Sync {
    fn create(&self, req: &CreateUser) -> impl Future<Output = Result<User, DomainError>> + Send;

    fn find_by_id(
        &self,
        id: &UserId,
    ) -> impl Future<Output = Result<Option<User>, DomainError>> + Send;

    fn find_by_email(
        &self,
        email: &str,
    ) -> impl Future<Output = Result<Option<User>, DomainError>> + Send;

    fn update_password(
        &self,
        id: &UserId,
        new_password: &str,
    ) -> impl Future<Output = Result<(), DomainError>> + Send;

    fn count(&self) -> impl Future<Output = Result<i64, DomainError>> + Send;

    fn soft_delete_user(
        &self,
        id: &UserId,
        actor_id: UserId,
    ) -> impl Future<Output = Result<User, DomainError>> + Send;

    fn update_user_role(
        &self,
        id: &UserId,
        new_role: RoleId,
        actor_id: UserId,
    ) -> impl Future<Output = Result<User, DomainError>> + Send;

    fn count_active_admins(&self) -> impl Future<Output = Result<u64, DomainError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_id_display() {
        let id = UserId::new(42);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn user_id_get_returns_inner() {
        let id = UserId::new(99);
        assert_eq!(id.get(), 99);
    }

    #[test]
    fn user_id_serialize_roundtrip() {
        let id = UserId::new(7);
        let json = serde_json::to_string(&id).unwrap();
        let restored: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, restored);
    }

    #[test]
    fn role_id_display() {
        let id = RoleId::new(1);
        assert_eq!(id.to_string(), "1");
    }

    #[test]
    fn role_id_get_returns_inner() {
        let id = RoleId::new(3);
        assert_eq!(id.get(), 3);
    }
}
