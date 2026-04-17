use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub email: String,
    pub name: String,
    pub password_hash: String,
    pub role_id: i64,
    pub is_active: bool,
    pub last_login_at: Option<String>,
    pub recovery_code_hash: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    /// Consecutive failed login attempts since last successful authentication.
    /// Reset to 0 on successful login.
    pub failed_login_attempts: i32,
    /// ISO-8601 UTC timestamp after which the account automatically unlocks.
    /// NULL when the account is not locked.
    pub locked_until: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
