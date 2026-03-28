use mokumo_core::activity::ActivityAction;
use mokumo_core::error::DomainError;
use mokumo_core::user::traits::UserRepository;
use mokumo_core::user::{CreateUser, RoleId, User, UserId};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, TransactionTrait,
};

use super::entity::{self, Entity as UserEntity};
use super::password;
use crate::sea_err;

impl From<entity::Model> for User {
    fn from(m: entity::Model) -> Self {
        User {
            id: UserId::new(m.id),
            email: m.email,
            name: m.name,
            role_id: RoleId::new(m.role_id),
            is_active: m.is_active,
            last_login_at: m.last_login_at,
            created_at: m.created_at,
            updated_at: m.updated_at,
            deleted_at: m.deleted_at,
        }
    }
}

async fn log_user_activity(
    conn: &impl ConnectionTrait,
    user: &User,
    action: ActivityAction,
) -> Result<(), DomainError> {
    let payload = serde_json::to_value(user).map_err(|e| DomainError::Internal {
        message: format!("failed to serialize user for activity log: {e}"),
    })?;
    crate::activity::insert_activity_log_raw(
        conn,
        "user",
        &user.id.to_string(),
        action,
        &user.id.to_string(),
        "user",
        &payload,
    )
    .await
}

fn split_user_and_hash(m: entity::Model) -> (User, String) {
    let hash = m.password_hash.clone();
    (User::from(m), hash)
}

fn is_sqlite_busy_domain(err: &DomainError) -> bool {
    matches!(err, DomainError::Internal { message } if message.contains("database is locked"))
}

/// Generate 10 recovery codes with argon2 hashes.
/// Returns (plaintext_codes, recovery_json_string).
async fn generate_recovery_codes() -> Result<(Vec<String>, String), DomainError> {
    let plaintext_codes: Vec<String> = {
        use rand::Rng;
        let mut rng = rand::rng();
        (0..10)
            .map(|_| {
                let mut code = String::with_capacity(9);
                for i in 0..8 {
                    if i == 4 {
                        code.push('-');
                    }
                    let c = match rng.random_range(0..36u8) {
                        n @ 0..=9 => (b'0' + n) as char,
                        n => (b'a' + n - 10) as char,
                    };
                    code.push(c);
                }
                code
            })
            .collect()
    };

    let mut hashed_codes = Vec::with_capacity(plaintext_codes.len());
    for code in &plaintext_codes {
        let hash = password::hash_password(code.replace('-', "")).await?;
        hashed_codes.push(serde_json::json!({"hash": hash, "used": false}));
    }
    let recovery_json =
        serde_json::to_string(&hashed_codes).map_err(|e| DomainError::Internal {
            message: format!("failed to serialize recovery codes: {e}"),
        })?;

    Ok((plaintext_codes, recovery_json))
}

pub struct SeaOrmUserRepo {
    db: DatabaseConnection,
}

impl SeaOrmUserRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn log_auth_activity(
        &self,
        user: &User,
        action: ActivityAction,
    ) -> Result<(), DomainError> {
        log_user_activity(&self.db, user, action).await
    }

    pub async fn find_by_email_with_hash(
        &self,
        email: &str,
    ) -> Result<Option<(User, String)>, DomainError> {
        let model = UserEntity::find()
            .filter(entity::Column::Email.eq(email))
            .filter(entity::Column::DeletedAt.is_null())
            .one(&self.db)
            .await
            .map_err(sea_err)?;

        Ok(model.map(split_user_and_hash))
    }

    pub async fn find_by_id_with_hash(
        &self,
        id: &UserId,
    ) -> Result<Option<(User, String)>, DomainError> {
        let model = UserEntity::find_by_id(id.get())
            .filter(entity::Column::DeletedAt.is_null())
            .one(&self.db)
            .await
            .map_err(sea_err)?;

        Ok(model.map(split_user_and_hash))
    }

    pub async fn create_admin_with_setup(
        &self,
        email: &str,
        name: &str,
        password: &str,
        shop_name: &str,
    ) -> Result<(User, Vec<String>), DomainError> {
        let password_hash = password::hash_password(password.to_string()).await?;
        let (plaintext_codes, recovery_json) = generate_recovery_codes().await?;

        let txn = self.db.begin().await.map_err(sea_err)?;

        let active = entity::ActiveModel {
            id: ActiveValue::NotSet,
            email: ActiveValue::Set(email.to_string()),
            name: ActiveValue::Set(name.to_string()),
            password_hash: ActiveValue::Set(password_hash),
            role_id: ActiveValue::Set(RoleId::ADMIN.get()),
            is_active: ActiveValue::Set(true),
            last_login_at: ActiveValue::NotSet,
            recovery_code_hash: ActiveValue::Set(Some(recovery_json)),
            created_at: ActiveValue::NotSet,
            updated_at: ActiveValue::NotSet,
            deleted_at: ActiveValue::NotSet,
        };

        let model = active.insert(&txn).await.map_err(sea_err)?;
        let user = User::from(model);

        txn.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "INSERT INTO settings (key, value) VALUES ('setup_complete', 'true')",
            vec![],
        ))
        .await
        .map_err(sea_err)?;

        txn.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('shop_name', ?)",
            vec![sea_orm::Value::from(shop_name.to_string())],
        ))
        .await
        .map_err(sea_err)?;

        log_user_activity(&txn, &user, ActivityAction::SetupCompleted).await?;

        txn.commit().await.map_err(sea_err)?;
        Ok((user, plaintext_codes))
    }

    pub async fn recovery_codes_remaining(&self, id: &UserId) -> Result<u32, DomainError> {
        let pool = self.db.get_sqlite_connection_pool();
        let json: Option<String> = sqlx::query_scalar(
            "SELECT recovery_code_hash FROM users WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id.get())
        .fetch_optional(pool)
        .await
        .map_err(|e| DomainError::Internal {
            message: format!("failed to query recovery codes: {e}"),
        })?
        .flatten();

        let json = match json {
            Some(j) => j,
            None => return Ok(0),
        };

        let codes: Vec<serde_json::Value> =
            serde_json::from_str(&json).map_err(|e| DomainError::Internal {
                message: format!("failed to parse recovery codes: {e}"),
            })?;

        let remaining = codes
            .iter()
            .filter(|entry| {
                entry
                    .get("used")
                    .and_then(|v| v.as_bool())
                    .map(|used| !used)
                    .unwrap_or(false)
            })
            .count();

        Ok(remaining as u32)
    }

    pub async fn regenerate_recovery_codes(&self, id: &UserId) -> Result<Vec<String>, DomainError> {
        let (plaintext_codes, recovery_json) = generate_recovery_codes().await?;

        let txn = self.db.begin().await.map_err(sea_err)?;

        let active = entity::ActiveModel {
            id: ActiveValue::Unchanged(id.get()),
            recovery_code_hash: ActiveValue::Set(Some(recovery_json)),
            ..Default::default()
        };
        active.update(&txn).await.map_err(sea_err)?;

        let user = User::from(
            UserEntity::find_by_id(id.get())
                .one(&txn)
                .await
                .map_err(sea_err)?
                .ok_or_else(|| DomainError::NotFound {
                    entity: "user",
                    id: id.to_string(),
                })?,
        );

        log_user_activity(&txn, &user, ActivityAction::RecoveryCodesRegenerated).await?;
        txn.commit().await.map_err(sea_err)?;

        Ok(plaintext_codes)
    }

    pub async fn verify_and_use_recovery_code(
        &self,
        email: &str,
        recovery_code: &str,
        new_password: &str,
    ) -> Result<bool, DomainError> {
        let normalized = recovery_code.replace('-', "");
        let new_hash = password::hash_password(new_password.to_string()).await?;

        for attempt in 0..3u64 {
            let result: Result<bool, DomainError> = async {
                let txn = self.db.begin().await.map_err(sea_err)?;

                let model = UserEntity::find()
                    .filter(entity::Column::Email.eq(email))
                    .filter(entity::Column::DeletedAt.is_null())
                    .one(&txn)
                    .await
                    .map_err(sea_err)?;

                let model = match model {
                    Some(m) => m,
                    None => return Ok(false),
                };

                let recovery_json = match &model.recovery_code_hash {
                    Some(json) => json.clone(),
                    None => return Ok(false),
                };

                let mut codes: Vec<serde_json::Value> = serde_json::from_str(&recovery_json)
                    .map_err(|e| DomainError::Internal {
                        message: format!("failed to parse recovery codes: {e}"),
                    })?;

                let mut matched_index = None;
                for (i, entry) in codes.iter().enumerate() {
                    let used = entry.get("used").and_then(|v| v.as_bool()).unwrap_or(true);
                    if used {
                        continue;
                    }
                    let hash = entry
                        .get("hash")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if password::verify_password(normalized.clone(), hash.to_string()).await? {
                        matched_index = Some(i);
                        break;
                    }
                }

                let matched_index = match matched_index {
                    Some(i) => i,
                    None => return Ok(false),
                };

                codes[matched_index]["used"] = serde_json::Value::Bool(true);
                let updated_json =
                    serde_json::to_string(&codes).map_err(|e| DomainError::Internal {
                        message: format!("failed to serialize updated recovery codes: {e}"),
                    })?;

                let update_result = UserEntity::update_many()
                    .col_expr(
                        entity::Column::PasswordHash,
                        sea_orm::sea_query::Expr::value(new_hash.clone()),
                    )
                    .col_expr(
                        entity::Column::RecoveryCodeHash,
                        sea_orm::sea_query::Expr::value(updated_json),
                    )
                    .filter(entity::Column::Id.eq(model.id))
                    .filter(entity::Column::RecoveryCodeHash.eq(recovery_json))
                    .exec(&txn)
                    .await
                    .map_err(sea_err)?;

                if update_result.rows_affected == 0 {
                    txn.rollback().await.map_err(sea_err)?;
                    return Ok(false);
                }

                let user = User::from(
                    UserEntity::find_by_id(model.id)
                        .one(&txn)
                        .await
                        .map_err(sea_err)?
                        .ok_or_else(|| DomainError::Internal {
                            message: "user disappeared mid-transaction".into(),
                        })?,
                );

                log_user_activity(&txn, &user, ActivityAction::PasswordReset).await?;
                txn.commit().await.map_err(sea_err)?;
                Ok(true)
            }
            .await;

            match result {
                Ok(val) => return Ok(val),
                Err(ref err) if is_sqlite_busy_domain(err) => {
                    if attempt < 2 {
                        let backoff_ms = 50 * (attempt + 1);
                        tracing::warn!(
                            attempt = attempt + 1,
                            backoff_ms,
                            "SQLITE_BUSY during recovery code verification, retrying"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    break;
                }
                Err(err) => return Err(err),
            }
        }

        Err(DomainError::Internal {
            message: "recovery code verification failed: database busy after retries".into(),
        })
    }
}

impl UserRepository for SeaOrmUserRepo {
    async fn create(&self, req: &CreateUser) -> Result<User, DomainError> {
        let password_hash = password::hash_password(req.password.clone()).await?;

        let txn = self.db.begin().await.map_err(sea_err)?;

        let active = entity::ActiveModel {
            id: ActiveValue::NotSet,
            email: ActiveValue::Set(req.email.clone()),
            name: ActiveValue::Set(req.name.clone()),
            password_hash: ActiveValue::Set(password_hash),
            role_id: ActiveValue::Set(req.role_id.get()),
            is_active: ActiveValue::Set(true),
            last_login_at: ActiveValue::NotSet,
            recovery_code_hash: ActiveValue::NotSet,
            created_at: ActiveValue::NotSet,
            updated_at: ActiveValue::NotSet,
            deleted_at: ActiveValue::NotSet,
        };

        let model = active.insert(&txn).await.map_err(sea_err)?;
        let user = User::from(model);

        log_user_activity(&txn, &user, ActivityAction::Created).await?;

        txn.commit().await.map_err(sea_err)?;
        Ok(user)
    }

    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, DomainError> {
        let model = UserEntity::find_by_id(id.get())
            .filter(entity::Column::DeletedAt.is_null())
            .one(&self.db)
            .await
            .map_err(sea_err)?;
        Ok(model.map(User::from))
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>, DomainError> {
        let model = UserEntity::find()
            .filter(entity::Column::Email.eq(email))
            .filter(entity::Column::DeletedAt.is_null())
            .one(&self.db)
            .await
            .map_err(sea_err)?;
        Ok(model.map(User::from))
    }

    async fn update_password(&self, id: &UserId, new_password: &str) -> Result<(), DomainError> {
        let new_hash = password::hash_password(new_password.to_string()).await?;

        let txn = self.db.begin().await.map_err(sea_err)?;

        let active = entity::ActiveModel {
            id: ActiveValue::Unchanged(id.get()),
            password_hash: ActiveValue::Set(new_hash),
            ..Default::default()
        };
        active.update(&txn).await.map_err(sea_err)?;

        let user = User::from(
            UserEntity::find_by_id(id.get())
                .one(&txn)
                .await
                .map_err(sea_err)?
                .ok_or_else(|| DomainError::NotFound {
                    entity: "user",
                    id: id.to_string(),
                })?,
        );

        log_user_activity(&txn, &user, ActivityAction::PasswordChanged).await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(())
    }

    async fn count(&self) -> Result<i64, DomainError> {
        let count = UserEntity::find()
            .filter(entity::Column::DeletedAt.is_null())
            .count(&self.db)
            .await
            .map_err(sea_err)?;
        Ok(count as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mokumo_core::user::traits::UserRepository;

    async fn test_db() -> (DatabaseConnection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = crate::initialize_database(&url).await.unwrap();
        (db, tmp)
    }

    #[tokio::test]
    async fn create_user_and_find_by_email() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let req = CreateUser {
            email: "admin@shop.local".to_string(),
            name: "Admin".to_string(),
            password: "testpassword123".to_string(),
            role_id: RoleId::new(1),
        };

        let user = repo.create(&req).await.unwrap();
        assert_eq!(user.email, "admin@shop.local");
        assert_eq!(user.name, "Admin");
        assert_eq!(user.role_id, RoleId::new(1));
        assert!(user.is_active);

        let found = repo.find_by_email("admin@shop.local").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, user.id);
    }

    #[tokio::test]
    async fn find_by_id_returns_user() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let req = CreateUser {
            email: "test@shop.local".to_string(),
            name: "Test".to_string(),
            password: "pass123".to_string(),
            role_id: RoleId::new(2),
        };

        let created = repo.create(&req).await.unwrap();
        let found = repo.find_by_id(&created.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().email, "test@shop.local");
    }

    #[tokio::test]
    async fn count_returns_active_users() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        assert_eq!(repo.count().await.unwrap(), 0);

        let req = CreateUser {
            email: "user1@shop.local".to_string(),
            name: "User 1".to_string(),
            password: "pass".to_string(),
            role_id: RoleId::new(1),
        };
        repo.create(&req).await.unwrap();

        assert_eq!(repo.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn update_password_changes_hash() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let req = CreateUser {
            email: "pw@shop.local".to_string(),
            name: "PW User".to_string(),
            password: "oldpass".to_string(),
            role_id: RoleId::new(1),
        };
        let user = repo.create(&req).await.unwrap();

        let (_, old_hash) = repo
            .find_by_email_with_hash("pw@shop.local")
            .await
            .unwrap()
            .unwrap();

        repo.update_password(&user.id, "newpass").await.unwrap();

        let (_, new_hash) = repo
            .find_by_email_with_hash("pw@shop.local")
            .await
            .unwrap()
            .unwrap();
        assert_ne!(old_hash, new_hash);
    }

    #[tokio::test]
    async fn find_by_email_with_hash_returns_hash() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let req = CreateUser {
            email: "hash@shop.local".to_string(),
            name: "Hash User".to_string(),
            password: "secret".to_string(),
            role_id: RoleId::new(1),
        };
        repo.create(&req).await.unwrap();

        let result = repo
            .find_by_email_with_hash("hash@shop.local")
            .await
            .unwrap();
        assert!(result.is_some());
        let (user, hash) = result.unwrap();
        assert_eq!(user.email, "hash@shop.local");
        assert!(!hash.is_empty());
        assert!(hash.starts_with("$argon2"));
    }

    #[tokio::test]
    async fn create_admin_with_setup_returns_recovery_codes() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());

        let (user, codes) = repo
            .create_admin_with_setup("admin@test.local", "Admin", "password123", "Test Shop")
            .await
            .unwrap();

        assert_eq!(user.email, "admin@test.local");
        assert_eq!(user.name, "Admin");
        assert_eq!(user.role_id, RoleId::new(1));
        assert_eq!(codes.len(), 10);

        for code in &codes {
            assert_eq!(code.len(), 9);
            assert_eq!(&code[4..5], "-");
            for (i, ch) in code.chars().enumerate() {
                if i == 4 {
                    assert_eq!(ch, '-');
                } else {
                    assert!(
                        ch.is_ascii_lowercase() || ch.is_ascii_digit(),
                        "Recovery code char '{ch}' at position {i} is not alphanumeric"
                    );
                }
            }
        }

        let is_complete = crate::is_setup_complete(&db).await.unwrap();
        assert!(is_complete);

        let pool = db.get_sqlite_connection_pool();
        let row: (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'shop_name'")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(row.0, "Test Shop");
    }

    #[tokio::test]
    async fn create_rolls_back_when_activity_log_fails() {
        let (db, _tmp) = test_db().await;
        let pool = db.get_sqlite_connection_pool().clone();

        sqlx::query("DROP TABLE activity_log")
            .execute(&pool)
            .await
            .unwrap();

        let repo = SeaOrmUserRepo::new(db);
        let req = CreateUser {
            email: "fault@test.local".to_string(),
            name: "Fault".to_string(),
            password: "pass".to_string(),
            role_id: RoleId::new(1),
        };

        let result = repo.create(&req).await;
        assert!(result.is_err());

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn find_by_id_with_hash_returns_user_and_hash() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let req = CreateUser {
            email: "idhash@shop.local".to_string(),
            name: "ID Hash User".to_string(),
            password: "secret123".to_string(),
            role_id: RoleId::new(1),
        };
        let created = repo.create(&req).await.unwrap();

        let result = repo.find_by_id_with_hash(&created.id).await.unwrap();
        assert!(result.is_some(), "find_by_id_with_hash should return Some");
        let (user, hash) = result.unwrap();
        assert_eq!(user.id, created.id);
        assert_eq!(user.email, "idhash@shop.local");
        assert!(hash.starts_with("$argon2"));

        let missing = repo
            .find_by_id_with_hash(&UserId::new(99999))
            .await
            .unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn verify_and_use_recovery_code_works() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let (_, codes) = repo
            .create_admin_with_setup("recover@test.local", "Admin", "oldpass", "Shop")
            .await
            .unwrap();

        let result = repo
            .verify_and_use_recovery_code("recover@test.local", &codes[0], "newpass")
            .await
            .unwrap();
        assert!(result);

        let (_, hash) = repo
            .find_by_email_with_hash("recover@test.local")
            .await
            .unwrap()
            .unwrap();
        assert!(
            password::verify_password("newpass".to_string(), hash)
                .await
                .unwrap()
        );

        let result = repo
            .verify_and_use_recovery_code("recover@test.local", &codes[0], "anotherpass")
            .await
            .unwrap();
        assert!(!result);

        let result = repo
            .verify_and_use_recovery_code("recover@test.local", &codes[1], "yetanotherpass")
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn verify_and_use_recovery_code_allows_only_one_concurrent_success() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());

        let (_, codes) = repo
            .create_admin_with_setup("recover@test.local", "Admin", "oldpass", "Shop")
            .await
            .unwrap();

        let code = codes[0].clone();
        let repo_a = SeaOrmUserRepo::new(db.clone());
        let repo_b = SeaOrmUserRepo::new(db.clone());

        let (result_a, result_b) = tokio::join!(
            repo_a.verify_and_use_recovery_code("recover@test.local", &code, "newpass-a"),
            repo_b.verify_and_use_recovery_code("recover@test.local", &code, "newpass-b"),
        );

        let result_a = result_a.unwrap();
        let result_b = result_b.unwrap();
        let success_count = [result_a, result_b].into_iter().filter(|ok| *ok).count();
        assert_eq!(success_count, 1, "recovery code should only succeed once");

        let (_, hash) = SeaOrmUserRepo::new(db)
            .find_by_email_with_hash("recover@test.local")
            .await
            .unwrap()
            .unwrap();

        let password_a = password::verify_password("newpass-a".to_string(), hash.clone())
            .await
            .unwrap();
        let password_b = password::verify_password("newpass-b".to_string(), hash)
            .await
            .unwrap();
        assert!(
            password_a ^ password_b,
            "exactly one concurrent password update should win"
        );
    }

    #[tokio::test]
    async fn verify_recovery_code_invalid_returns_false() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        repo.create_admin_with_setup("inv@test.local", "Admin", "pass", "Shop")
            .await
            .unwrap();

        let result = repo
            .verify_and_use_recovery_code("inv@test.local", "xxxx-yyyy", "newpass")
            .await
            .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn verify_recovery_code_nonexistent_email_returns_false() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let result = repo
            .verify_and_use_recovery_code("nobody@test.local", "xxxx-yyyy", "newpass")
            .await
            .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn migration_creates_roles_seed_data() {
        let (db, _tmp) = test_db().await;
        let pool = db.get_sqlite_connection_pool();

        let roles: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM roles ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap();

        assert_eq!(roles.len(), 3);
        assert_eq!(roles[0], (1, "Admin".to_string()));
        assert_eq!(roles[1], (2, "Staff".to_string()));
        assert_eq!(roles[2], (3, "Guest".to_string()));
    }

    #[tokio::test]
    async fn regenerate_recovery_codes_returns_new_codes() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        repo.create_admin_with_setup("regen@test.local", "Admin", "password123", "Shop")
            .await
            .unwrap();

        let user = repo
            .find_by_email("regen@test.local")
            .await
            .unwrap()
            .unwrap();
        let new_codes = repo.regenerate_recovery_codes(&user.id).await.unwrap();

        assert_eq!(new_codes.len(), 10);
        for code in &new_codes {
            assert_eq!(code.len(), 9);
            assert_eq!(&code[4..5], "-");
        }
    }

    #[tokio::test]
    async fn regenerate_recovery_codes_invalidates_old() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let (_, original_codes) = repo
            .create_admin_with_setup("regen2@test.local", "Admin", "password123", "Shop")
            .await
            .unwrap();

        let user = repo
            .find_by_email("regen2@test.local")
            .await
            .unwrap()
            .unwrap();
        repo.regenerate_recovery_codes(&user.id).await.unwrap();

        // Old code should no longer work
        let result = repo
            .verify_and_use_recovery_code("regen2@test.local", &original_codes[0], "newpass")
            .await
            .unwrap();
        assert!(
            !result,
            "old recovery code should be invalidated after regeneration"
        );
    }

    #[tokio::test]
    async fn regenerate_recovery_codes_new_codes_work() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        repo.create_admin_with_setup("regen3@test.local", "Admin", "password123", "Shop")
            .await
            .unwrap();

        let user = repo
            .find_by_email("regen3@test.local")
            .await
            .unwrap()
            .unwrap();
        let new_codes = repo.regenerate_recovery_codes(&user.id).await.unwrap();

        // New code should work
        let result = repo
            .verify_and_use_recovery_code("regen3@test.local", &new_codes[0], "newpass")
            .await
            .unwrap();
        assert!(result, "new recovery code should work after regeneration");
    }

    #[tokio::test]
    async fn regenerate_recovery_codes_logs_activity() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());

        repo.create_admin_with_setup("regen4@test.local", "Admin", "password123", "Shop")
            .await
            .unwrap();

        let user = repo
            .find_by_email("regen4@test.local")
            .await
            .unwrap()
            .unwrap();
        repo.regenerate_recovery_codes(&user.id).await.unwrap();

        let pool = db.get_sqlite_connection_pool();
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM activity_log WHERE action = 'recovery_codes_regenerated'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            row.0, 1,
            "should have one recovery_codes_regenerated activity entry"
        );
    }

    #[tokio::test]
    async fn recovery_codes_remaining_initial() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let (user, _) = repo
            .create_admin_with_setup("remain@test.local", "Admin", "password123", "Shop")
            .await
            .unwrap();

        let count = repo.recovery_codes_remaining(&user.id).await.unwrap();
        assert_eq!(count, 10);
    }

    #[tokio::test]
    async fn recovery_codes_remaining_after_use() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let (user, codes) = repo
            .create_admin_with_setup("remain2@test.local", "Admin", "password123", "Shop")
            .await
            .unwrap();

        repo.verify_and_use_recovery_code("remain2@test.local", &codes[0], "newpass")
            .await
            .unwrap();

        let count = repo.recovery_codes_remaining(&user.id).await.unwrap();
        assert_eq!(count, 9);
    }

    #[tokio::test]
    async fn recovery_codes_remaining_after_regen() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let (user, codes) = repo
            .create_admin_with_setup("remain3@test.local", "Admin", "password123", "Shop")
            .await
            .unwrap();

        // Use 3 codes
        for code in &codes[0..3] {
            repo.verify_and_use_recovery_code("remain3@test.local", code, "pass")
                .await
                .unwrap();
        }
        assert_eq!(repo.recovery_codes_remaining(&user.id).await.unwrap(), 7);

        // Regenerate — should be back to 10
        repo.regenerate_recovery_codes(&user.id).await.unwrap();
        let count = repo.recovery_codes_remaining(&user.id).await.unwrap();
        assert_eq!(count, 10);
    }

    #[tokio::test]
    async fn users_updated_at_trigger_fires() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let req = CreateUser {
            email: "trigger@shop.local".to_string(),
            name: "Trigger Test".to_string(),
            password: "pass".to_string(),
            role_id: RoleId::new(1),
        };
        let user = repo.create(&req).await.unwrap();
        let original_updated = user.updated_at.clone();

        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

        repo.update_password(&user.id, "newpass").await.unwrap();

        let updated_user = repo.find_by_id(&user.id).await.unwrap().unwrap();
        assert_ne!(updated_user.updated_at, original_updated);
    }
}
