use super::domain::UserRepository;
use super::domain::{CreateUser, RoleId, User, UserId};
use crate::error::DomainError;
use kikan_types::activity::ActivityAction;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, TransactionTrait,
};

use super::entity_user::{self as entity, Entity as UserEntity};
use super::password;
use crate::control_plane_error::{ConflictKind, ControlPlaneError};
fn sea_err(e: sea_orm::DbErr) -> DomainError {
    DomainError::Internal {
        message: e.to_string(),
    }
}

/// Error type for the first-admin bootstrap composite method.
///
/// Encodes the `ALREADY_BOOTSTRAPPED` conflict explicitly so handlers can
/// render it as `ControlPlaneError::Conflict(ConflictKind::AlreadyBootstrapped)`
/// without string-sniffing a generic `DomainError::Conflict`. Any other
/// failure flows through `Domain`.
#[derive(Debug)]
pub enum BootstrapError {
    /// Another admin account already exists; bootstrap refused.
    AlreadyBootstrapped,
    /// Underlying domain/persistence failure.
    Domain(DomainError),
}

impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyBootstrapped => write!(f, "an admin account is already configured"),
            Self::Domain(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BootstrapError {}

impl From<DomainError> for BootstrapError {
    fn from(e: DomainError) -> Self {
        Self::Domain(e)
    }
}

impl From<BootstrapError> for ControlPlaneError {
    fn from(err: BootstrapError) -> Self {
        match err {
            BootstrapError::AlreadyBootstrapped => {
                ControlPlaneError::Conflict(ConflictKind::AlreadyBootstrapped)
            }
            BootstrapError::Domain(DomainError::NotFound { .. }) => ControlPlaneError::NotFound,
            BootstrapError::Domain(DomainError::Conflict { message }) => {
                ControlPlaneError::Validation {
                    field: "request".into(),
                    message,
                }
            }
            BootstrapError::Domain(DomainError::Validation { details }) => {
                // HashMap iteration order is non-deterministic; sort by key so
                // repeated conversions with the same input always pick the
                // same field. Keeps handler output reproducible and tests
                // stable when a composite path later surfaces multiple
                // field-level errors.
                let mut entries: Vec<_> = details.into_iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let (field, message) = entries
                    .into_iter()
                    .next()
                    .map(|(f, msgs)| (f, msgs.into_iter().next().unwrap_or_default()))
                    .unwrap_or_else(|| ("request".into(), "validation failed".into()));
                ControlPlaneError::Validation { field, message }
            }
            BootstrapError::Domain(DomainError::Internal { message }) => {
                ControlPlaneError::Internal(anyhow::anyhow!(message))
            }
        }
    }
}

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

const DELETE_LAST_ADMIN_MSG: &str =
    "Cannot delete the last admin account. Assign another admin first.";
const DEMOTE_LAST_ADMIN_MSG: &str =
    "Cannot demote the last admin account. Assign another admin first.";

async fn find_active_user_in_txn<C: ConnectionTrait>(
    conn: &C,
    id: &UserId,
) -> Result<entity::Model, DomainError> {
    UserEntity::find_by_id(id.get())
        .filter(entity::Column::DeletedAt.is_null())
        .one(conn)
        .await
        .map_err(sea_err)?
        .ok_or_else(|| DomainError::NotFound {
            entity: "user",
            id: id.to_string(),
        })
}

async fn reload_user_in_txn<C: ConnectionTrait>(
    conn: &C,
    id: &UserId,
) -> Result<User, DomainError> {
    UserEntity::find_by_id(id.get())
        .one(conn)
        .await
        .map_err(sea_err)?
        .map(User::from)
        .ok_or_else(|| DomainError::Internal {
            message: "user disappeared mid-transaction".into(),
        })
}

/// Guard: if the target is an admin and the operation would remove them
/// (soft-delete when `new_role` is None, or demotion when `new_role != ADMIN`),
/// reject unless another active admin remains.
async fn ensure_not_last_admin<C: ConnectionTrait>(
    conn: &C,
    current_role: RoleId,
    new_role: Option<RoleId>,
    message: &str,
) -> Result<(), DomainError> {
    let removes_admin =
        current_role == RoleId::ADMIN && new_role.map(|r| r != RoleId::ADMIN).unwrap_or(true);
    if !removes_admin {
        return Ok(());
    }
    let count = UserEntity::find()
        .filter(entity::Column::RoleId.eq(RoleId::ADMIN.get()))
        .filter(entity::Column::DeletedAt.is_null())
        .count(conn)
        .await
        .map_err(sea_err)?;
    if count <= 1 {
        return Err(DomainError::Conflict {
            message: message.into(),
        });
    }
    Ok(())
}

async fn log_user_activity(
    conn: &impl ConnectionTrait,
    user: &User,
    action: ActivityAction,
) -> Result<(), DomainError> {
    log_user_activity_with_actor(conn, user, action, user.id).await
}

/// Activity log helper used when the actor is a different user than the target
/// (e.g., an admin deleting or demoting another user).
async fn log_user_activity_with_actor(
    conn: &impl ConnectionTrait,
    user: &User,
    action: ActivityAction,
    actor_id: UserId,
) -> Result<(), DomainError> {
    let payload = serde_json::to_value(user).map_err(|e| DomainError::Internal {
        message: format!("failed to serialize user for activity log: {e}"),
    })?;
    crate::activity::insert_activity_log_raw(
        conn,
        "user",
        &user.id.to_string(),
        action,
        &actor_id.to_string(),
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
    generate_recovery_codes_n(10).await
}

/// Generate `n` recovery codes with argon2 hashes.
async fn generate_recovery_codes_n(n: u32) -> Result<(Vec<String>, String), DomainError> {
    let plaintext_codes: Vec<String> = {
        use rand::RngExt;
        let mut rng = rand::rng();
        (0..n)
            .map(|_| {
                let mut code = String::with_capacity(9);
                for i in 0..8 {
                    if i == 4 {
                        code.push('-');
                    }
                    let c = match rng.random_range(0..36u8) {
                        v @ 0..=9 => (b'0' + v) as char,
                        v => (b'a' + v - 10) as char,
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

/// Mark the install as setup-complete by writing `settings.setup_complete = 'true'`.
///
/// Both first-admin paths (HTTP setup wizard via `create_admin_with_setup`
/// and CLI bootstrap via `bootstrap_admin_with_codes`) must leave the install
/// in the same `setup_complete=true` state. Calling this inside the same
/// transaction that creates the admin keeps the two writes atomic — a crash
/// cannot leave an admin row without the matching settings row.
///
/// Idempotent (`INSERT OR REPLACE`): a stray pre-existing `setup_complete`
/// row from a partial setup, restore-from-backup, or manual DB edit converges
/// to the intended state instead of failing the transaction with a UNIQUE
/// constraint violation.
async fn mark_setup_complete<C: ConnectionTrait>(c: &C) -> Result<(), sea_orm::DbErr> {
    c.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Sqlite,
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('setup_complete', 'true')",
        vec![],
    ))
    .await
    .map(|_| ())
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
            failed_login_attempts: ActiveValue::Set(0),
            locked_until: ActiveValue::Set(None),
        };

        let model = active.insert(&txn).await.map_err(sea_err)?;
        let user = User::from(model);

        mark_setup_complete(&txn).await.map_err(sea_err)?;

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

        Ok(u32::try_from(remaining).unwrap_or(u32::MAX))
    }

    pub async fn regenerate_recovery_codes(&self, id: &UserId) -> Result<Vec<String>, DomainError> {
        let (plaintext_codes, recovery_json) = generate_recovery_codes().await?;

        let txn = self.db.begin().await.map_err(sea_err)?;

        let result = txn
            .execute_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                "UPDATE users SET recovery_code_hash = ? WHERE id = ? AND deleted_at IS NULL",
                vec![
                    sea_orm::Value::from(recovery_json),
                    sea_orm::Value::from(id.get()),
                ],
            ))
            .await
            .map_err(sea_err)?;

        if result.rows_affected() == 0 {
            txn.rollback().await.map_err(sea_err)?;
            return Err(DomainError::NotFound {
                entity: "user",
                id: id.to_string(),
            });
        }

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

    /// Return the user's ID and current `locked_until` timestamp for the given email.
    ///
    /// Used by the login handler to pre-check account lockout before running
    /// the expensive argon2 password hash. Returns `None` if the email is not
    /// found or the user is soft-deleted.
    pub async fn find_lockout_state_by_email(
        &self,
        email: &str,
    ) -> Result<Option<(UserId, Option<String>)>, DomainError> {
        let pool = self.db.get_sqlite_connection_pool();
        let row: Option<(i64, Option<String>)> = sqlx::query_as(
            "SELECT id, locked_until FROM users WHERE email = ? AND deleted_at IS NULL",
        )
        .bind(email)
        .fetch_optional(pool)
        .await
        .map_err(|e| DomainError::Internal {
            message: format!("failed to query lockout state: {e}"),
        })?;

        Ok(row.map(|(id, locked_until)| (UserId::new(id), locked_until)))
    }

    /// Atomically increment `failed_login_attempts` and log a `LoginFailed` or
    /// `AccountLocked` activity entry in the same transaction. If the new count
    /// reaches `threshold` and the account is not already locked, set
    /// `locked_until` to `now + lock_secs` seconds.
    ///
    /// Returns `(new_count, locked_until)`. Callers should check whether
    /// `locked_until` is `Some` to decide whether to return HTTP 423.
    ///
    /// If the account is already locked, the counter is not incremented further
    /// and the existing `locked_until` is returned unchanged.
    pub async fn record_failed_attempt(
        &self,
        user_id: UserId,
        threshold: i32,
        lock_secs: i64,
    ) -> Result<(i32, Option<String>), DomainError> {
        use chrono::{Duration, Utc};

        let lock_until_ts = Utc::now()
            .checked_add_signed(Duration::seconds(lock_secs))
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .ok_or_else(|| DomainError::Internal {
                message: "lock_secs out of range".into(),
            })?;

        let txn = self.db.begin().await.map_err(sea_err)?;

        let before = UserEntity::find_by_id(user_id.get())
            .filter(entity::Column::DeletedAt.is_null())
            .one(&txn)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| DomainError::NotFound {
                entity: "user",
                id: user_id.to_string(),
            })?;

        txn.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "UPDATE users
                SET
                    failed_login_attempts = CASE
                        WHEN locked_until IS NOT NULL
                             AND locked_until > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                        THEN failed_login_attempts
                        ELSE failed_login_attempts + 1
                    END,
                    locked_until = CASE
                        WHEN locked_until IS NOT NULL
                             AND locked_until > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                        THEN locked_until
                        WHEN (failed_login_attempts + 1) >= ?
                        THEN ?
                        ELSE NULL
                    END
             WHERE id = ? AND deleted_at IS NULL",
            vec![
                sea_orm::Value::from(threshold),
                sea_orm::Value::from(lock_until_ts),
                sea_orm::Value::from(user_id.get()),
            ],
        ))
        .await
        .map_err(sea_err)?;

        let after = UserEntity::find_by_id(user_id.get())
            .one(&txn)
            .await
            .map_err(sea_err)?
            .ok_or_else(|| DomainError::Internal {
                message: "user disappeared mid-transaction".into(),
            })?;
        let count = after.failed_login_attempts;
        let locked_until = after.locked_until.clone();
        let user = User::from(after);

        // Audit event fires *inside* the same transaction that mutates the
        // counter so a commit failure rolls back both — satisfying the
        // adapter-enforced activity-logging contract. We log AccountLocked only
        // when this attempt transitioned the account into the locked state
        // (counter advanced past threshold); repeat attempts against an already
        // locked account still log LoginFailed.
        let counter_advanced = before.failed_login_attempts != count;
        let action = if counter_advanced && count >= threshold && locked_until.is_some() {
            ActivityAction::AccountLocked
        } else {
            ActivityAction::LoginFailed
        };
        log_user_activity(&txn, &user, action).await?;

        txn.commit().await.map_err(sea_err)?;

        Ok((count, locked_until))
    }

    /// Reset `failed_login_attempts` to 0 and clear `locked_until`.
    ///
    /// Called on successful login to restore normal authentication for the
    /// account. Does not log any activity — callers use `log_auth_activity`
    /// for `LoginSuccess`.
    pub async fn clear_failed_attempts(&self, user_id: UserId) -> Result<(), DomainError> {
        let pool = self.db.get_sqlite_connection_pool();
        let rows_affected = sqlx::query(
            "UPDATE users SET failed_login_attempts = 0, locked_until = NULL WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(user_id.get())
        .execute(pool)
        .await
        .map_err(|e| DomainError::Internal {
            message: format!("failed to clear failed attempts: {e}"),
        })?
        .rows_affected();

        if rows_affected == 0 {
            return Err(DomainError::NotFound {
                entity: "user",
                id: user_id.to_string(),
            });
        }
        Ok(())
    }

    /// Admin unlock: reset `failed_login_attempts` to 0 and clear `locked_until`,
    /// then log `ActivityAction::AccountUnlocked` with the admin as actor.
    ///
    /// Intended for Tauri IPC Stage 4 to wire an `unlock_user` admin command.
    pub async fn unlock_user(&self, user_id: UserId, actor_id: UserId) -> Result<(), DomainError> {
        let txn = self.db.begin().await.map_err(sea_err)?;

        let result = txn
            .execute_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                "UPDATE users SET failed_login_attempts = 0, locked_until = NULL WHERE id = ? AND deleted_at IS NULL",
                vec![sea_orm::Value::from(user_id.get())],
            ))
            .await
            .map_err(sea_err)?;

        if result.rows_affected() == 0 {
            txn.rollback().await.map_err(sea_err)?;
            return Err(DomainError::NotFound {
                entity: "user",
                id: user_id.to_string(),
            });
        }

        let user = reload_user_in_txn(&txn, &user_id).await?;

        log_user_activity_with_actor(&txn, &user, ActivityAction::AccountUnlocked, actor_id)
            .await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(())
    }

    /// Create a user and attach a batch of hashed recovery codes in a single
    /// transaction, logging an `ActivityAction::Created` entry on the same
    /// connection. Either every row (user + codes + activity) persists or
    /// none do.
    ///
    /// The caller specifies `codes_count` so the method can surface an
    /// explicit `DomainError::Validation` when the batch is rejected — a
    /// real validation seam that covers the `user_repo_atomicity.feature`
    /// "recovery code batch that fails validation" scenario.
    pub async fn create_user_with_codes(
        &self,
        req: &CreateUser,
        codes_count: u32,
    ) -> Result<(User, Vec<String>), DomainError> {
        if codes_count == 0 || codes_count > 16 {
            let mut details = std::collections::HashMap::new();
            details.insert(
                "recovery_codes".to_string(),
                vec![format!(
                    "codes_count must be between 1 and 16; got {codes_count}"
                )],
            );
            return Err(DomainError::Validation { details });
        }

        let password_hash = password::hash_password(req.password.clone()).await?;
        let (plaintext_codes, recovery_json) = generate_recovery_codes_n(codes_count).await?;

        let txn = self.db.begin().await.map_err(sea_err)?;

        let active = entity::ActiveModel {
            id: ActiveValue::NotSet,
            email: ActiveValue::Set(req.email.clone()),
            name: ActiveValue::Set(req.name.clone()),
            password_hash: ActiveValue::Set(password_hash),
            role_id: ActiveValue::Set(req.role_id.get()),
            is_active: ActiveValue::Set(true),
            last_login_at: ActiveValue::NotSet,
            recovery_code_hash: ActiveValue::Set(Some(recovery_json)),
            created_at: ActiveValue::NotSet,
            updated_at: ActiveValue::NotSet,
            deleted_at: ActiveValue::NotSet,
            failed_login_attempts: ActiveValue::Set(0),
            locked_until: ActiveValue::Set(None),
        };

        let model = active.insert(&txn).await.map_err(sea_err)?;
        let user = User::from(model);

        log_user_activity(&txn, &user, ActivityAction::Created).await?;

        txn.commit().await.map_err(sea_err)?;
        Ok((user, plaintext_codes))
    }

    /// Bootstrap the first admin account: atomically create the admin user +
    /// 10 recovery codes + `ActivityAction::Bootstrap` entry, but only if no
    /// active admin exists.
    ///
    /// The guard runs inside the transaction so sequential callers race-safely
    /// — a second call on a non-empty admin set rolls back and returns
    /// `BootstrapError::AlreadyBootstrapped`. This is NOT, however, safe under
    /// *concurrent* callers: SQLite's default `DEFERRED` transaction takes a
    /// write lock at the first write, not at `BEGIN`, so two in-flight
    /// bootstraps could both observe `existing_admins == 0` before either
    /// inserts. In the current deployment model bootstrap is a single-shot
    /// first-run operation protected by application-level coordination
    /// (`setup_token`, `setup_in_progress`), so the DEFERRED race is
    /// unreachable. Adding a partial unique index on `(role_id, deleted_at)`
    /// is tracked as a follow-up if the threat model ever changes.
    pub async fn bootstrap_admin_with_codes(
        &self,
        email: &str,
        name: &str,
        password: &str,
    ) -> Result<(User, Vec<String>), BootstrapError> {
        let password_hash = password::hash_password(password.to_string())
            .await
            .map_err(BootstrapError::Domain)?;
        let (plaintext_codes, recovery_json) = generate_recovery_codes_n(10)
            .await
            .map_err(BootstrapError::Domain)?;

        let txn = self
            .db
            .begin()
            .await
            .map_err(sea_err)
            .map_err(BootstrapError::Domain)?;

        let existing_admins = UserEntity::find()
            .filter(entity::Column::RoleId.eq(RoleId::ADMIN.get()))
            .filter(entity::Column::DeletedAt.is_null())
            .count(&txn)
            .await
            .map_err(sea_err)
            .map_err(BootstrapError::Domain)?;

        if existing_admins > 0 {
            txn.rollback()
                .await
                .map_err(sea_err)
                .map_err(BootstrapError::Domain)?;
            return Err(BootstrapError::AlreadyBootstrapped);
        }

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
            failed_login_attempts: ActiveValue::Set(0),
            locked_until: ActiveValue::Set(None),
        };

        let model = active
            .insert(&txn)
            .await
            .map_err(sea_err)
            .map_err(BootstrapError::Domain)?;
        let user = User::from(model);

        log_user_activity(&txn, &user, ActivityAction::Bootstrap)
            .await
            .map_err(BootstrapError::Domain)?;

        mark_setup_complete(&txn)
            .await
            .map_err(sea_err)
            .map_err(BootstrapError::Domain)?;

        txn.commit()
            .await
            .map_err(sea_err)
            .map_err(BootstrapError::Domain)?;
        Ok((user, plaintext_codes))
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
            failed_login_attempts: ActiveValue::Set(0),
            locked_until: ActiveValue::Set(None),
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

        let result = txn
            .execute_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                "UPDATE users SET password_hash = ? WHERE id = ? AND deleted_at IS NULL",
                vec![
                    sea_orm::Value::from(new_hash),
                    sea_orm::Value::from(id.get()),
                ],
            ))
            .await
            .map_err(sea_err)?;

        if result.rows_affected() == 0 {
            txn.rollback().await.map_err(sea_err)?;
            return Err(DomainError::NotFound {
                entity: "user",
                id: id.to_string(),
            });
        }

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
        Ok(i64::try_from(count).unwrap_or(i64::MAX))
    }

    async fn count_active_admins(&self) -> Result<u64, DomainError> {
        let count = UserEntity::find()
            .filter(entity::Column::RoleId.eq(RoleId::ADMIN.get()))
            .filter(entity::Column::DeletedAt.is_null())
            .count(&self.db)
            .await
            .map_err(sea_err)?;
        Ok(count)
    }

    async fn soft_delete_user(&self, id: &UserId, actor_id: UserId) -> Result<User, DomainError> {
        let txn = self.db.begin().await.map_err(sea_err)?;
        let model = find_active_user_in_txn(&txn, id).await?;
        ensure_not_last_admin(
            &txn,
            RoleId::new(model.role_id),
            None,
            DELETE_LAST_ADMIN_MSG,
        )
        .await?;

        let mut active: entity::ActiveModel = model.into();
        active.deleted_at = ActiveValue::Set(Some(chrono::Utc::now().to_rfc3339()));
        active.is_active = ActiveValue::Set(false);
        active.update(&txn).await.map_err(sea_err)?;

        let user = reload_user_in_txn(&txn, id).await?;
        log_user_activity_with_actor(&txn, &user, ActivityAction::SoftDeleted, actor_id).await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(user)
    }

    async fn update_user_role(
        &self,
        id: &UserId,
        new_role: RoleId,
        actor_id: UserId,
    ) -> Result<User, DomainError> {
        let txn = self.db.begin().await.map_err(sea_err)?;
        let model = find_active_user_in_txn(&txn, id).await?;
        ensure_not_last_admin(
            &txn,
            RoleId::new(model.role_id),
            Some(new_role),
            DEMOTE_LAST_ADMIN_MSG,
        )
        .await?;

        let mut active: entity::ActiveModel = model.into();
        active.role_id = ActiveValue::Set(new_role.get());
        active.update(&txn).await.map_err(sea_err)?;

        let user = reload_user_in_txn(&txn, id).await?;
        log_user_activity_with_actor(&txn, &user, ActivityAction::RoleUpdated, actor_id).await?;
        txn.commit().await.map_err(sea_err)?;
        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::domain::UserRepository;

    async fn test_db() -> (DatabaseConnection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let db = mokumo_shop::db::initialize_database(&url).await.unwrap();
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
            .create_admin_with_setup("admin@test.local", "Admin", "password123")
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

        let is_complete = mokumo_shop::db::is_setup_complete(&db).await.unwrap();
        assert!(is_complete);
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
            .create_admin_with_setup("recover@test.local", "Admin", "oldpass")
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
            .create_admin_with_setup("recover@test.local", "Admin", "oldpass")
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

        repo.create_admin_with_setup("inv@test.local", "Admin", "pass")
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

        repo.create_admin_with_setup("regen@test.local", "Admin", "password123")
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
            .create_admin_with_setup("regen2@test.local", "Admin", "password123")
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

        repo.create_admin_with_setup("regen3@test.local", "Admin", "password123")
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

        repo.create_admin_with_setup("regen4@test.local", "Admin", "password123")
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
            .create_admin_with_setup("remain@test.local", "Admin", "password123")
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
            .create_admin_with_setup("remain2@test.local", "Admin", "password123")
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
            .create_admin_with_setup("remain3@test.local", "Admin", "password123")
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

    fn admin_req(email: &str) -> CreateUser {
        CreateUser {
            email: email.to_string(),
            name: "Admin".to_string(),
            password: "pass123".to_string(),
            role_id: RoleId::ADMIN,
        }
    }

    fn staff_req(email: &str) -> CreateUser {
        CreateUser {
            email: email.to_string(),
            name: "Staff".to_string(),
            password: "pass123".to_string(),
            role_id: RoleId::STAFF,
        }
    }

    #[tokio::test]
    async fn soft_delete_user_removes_target() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let admin = repo.create(&admin_req("admin@shop.local")).await.unwrap();
        let staff = repo.create(&staff_req("staff@shop.local")).await.unwrap();

        let result = repo.soft_delete_user(&staff.id, admin.id).await;
        assert!(result.is_ok());
        let deleted = result.unwrap();
        assert!(deleted.deleted_at.is_some());

        // Verify it's gone from active queries
        let found = repo.find_by_id(&staff.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn soft_delete_user_last_admin_guard_fires() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let admin = repo.create(&admin_req("admin@shop.local")).await.unwrap();

        let result = repo.soft_delete_user(&admin.id, admin.id).await;
        assert!(
            matches!(result, Err(DomainError::Conflict { ref message }) if
            message.contains("Cannot delete the last admin account"))
        );

        // Admin must still be active
        let still_there = repo.find_by_id(&admin.id).await.unwrap();
        assert!(still_there.is_some());
        assert!(still_there.unwrap().deleted_at.is_none());
    }

    // R6 boundary: a soft-deleted admin must NOT count — guard fires even when ghost admin exists
    #[tokio::test]
    async fn soft_delete_user_ghost_admin_not_counted() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let admin1 = repo.create(&admin_req("admin1@shop.local")).await.unwrap();
        let admin2 = repo.create(&admin_req("admin2@shop.local")).await.unwrap();

        // Soft-delete admin2 to create a "ghost admin"
        repo.soft_delete_user(&admin2.id, admin1.id).await.unwrap();

        // Now admin1 is the only *active* admin; deleting admin1 should fail
        let result = repo.soft_delete_user(&admin1.id, admin1.id).await;
        assert!(
            matches!(result, Err(DomainError::Conflict { ref message }) if
            message.contains("Cannot delete the last admin account"))
        );
    }

    #[tokio::test]
    async fn update_user_role_demotes_target() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let admin1 = repo.create(&admin_req("admin1@shop.local")).await.unwrap();
        let admin2 = repo.create(&admin_req("admin2@shop.local")).await.unwrap();

        let result = repo
            .update_user_role(&admin2.id, RoleId::STAFF, admin1.id)
            .await;
        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.role_id, RoleId::STAFF);
    }

    #[tokio::test]
    async fn update_user_role_demote_last_admin_guard_fires() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let admin = repo.create(&admin_req("admin@shop.local")).await.unwrap();

        let result = repo
            .update_user_role(&admin.id, RoleId::STAFF, admin.id)
            .await;
        assert!(
            matches!(result, Err(DomainError::Conflict { ref message }) if
            message.contains("Cannot demote the last admin account"))
        );

        // Role must be unchanged
        let still_admin = repo.find_by_id(&admin.id).await.unwrap().unwrap();
        assert_eq!(still_admin.role_id, RoleId::ADMIN);
    }

    // R6 boundary: ghost admin must not count for demote guard
    #[tokio::test]
    async fn update_user_role_ghost_admin_not_counted() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let admin1 = repo.create(&admin_req("admin1@shop.local")).await.unwrap();
        let admin2 = repo.create(&admin_req("admin2@shop.local")).await.unwrap();

        // Soft-delete admin2 to create a ghost admin
        repo.soft_delete_user(&admin2.id, admin1.id).await.unwrap();

        // Demoting admin1 (the only active admin) must fail
        let result = repo
            .update_user_role(&admin1.id, RoleId::STAFF, admin1.id)
            .await;
        assert!(
            matches!(result, Err(DomainError::Conflict { ref message }) if
            message.contains("Cannot demote the last admin account"))
        );
    }

    // --- Lockout methods ---

    async fn create_test_user(repo: &SeaOrmUserRepo, email: &str) -> UserId {
        let req = CreateUser {
            email: email.to_string(),
            name: "Test".to_string(),
            password: "pass123".to_string(),
            role_id: RoleId::new(1),
        };
        repo.create(&req).await.unwrap().id
    }

    #[tokio::test]
    async fn find_lockout_state_by_email_returns_none_for_unknown() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let result = repo
            .find_lockout_state_by_email("nobody@shop.local")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn find_lockout_state_by_email_returns_id_and_null_locked_until() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);
        let user_id = create_test_user(&repo, "lock@shop.local").await;

        let result = repo
            .find_lockout_state_by_email("lock@shop.local")
            .await
            .unwrap();
        assert!(result.is_some());
        let (found_id, locked_until) = result.unwrap();
        assert_eq!(found_id, user_id);
        assert!(locked_until.is_none(), "new account should not be locked");
    }

    #[tokio::test]
    async fn record_failed_attempt_under_threshold_does_not_lock() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);
        let user_id = create_test_user(&repo, "fa@shop.local").await;

        // threshold=3, 2 attempts → not locked
        let (count1, locked1) = repo.record_failed_attempt(user_id, 3, 900).await.unwrap();
        assert_eq!(count1, 1);
        assert!(locked1.is_none());

        let (count2, locked2) = repo.record_failed_attempt(user_id, 3, 900).await.unwrap();
        assert_eq!(count2, 2);
        assert!(locked2.is_none());
    }

    #[tokio::test]
    async fn record_failed_attempt_at_threshold_locks_account() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);
        let user_id = create_test_user(&repo, "lock2@shop.local").await;

        // 2 attempts below threshold
        repo.record_failed_attempt(user_id, 3, 900).await.unwrap();
        repo.record_failed_attempt(user_id, 3, 900).await.unwrap();

        // 3rd attempt hits threshold → locked
        let (count, locked_until) = repo.record_failed_attempt(user_id, 3, 900).await.unwrap();
        assert_eq!(count, 3);
        assert!(
            locked_until.is_some(),
            "account should be locked at threshold"
        );
    }

    #[tokio::test]
    async fn record_failed_attempt_when_already_locked_does_not_advance_counter() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);
        let user_id = create_test_user(&repo, "lock3@shop.local").await;

        // Lock the account at threshold=1
        let (count_at_lock, locked_until) =
            repo.record_failed_attempt(user_id, 1, 900).await.unwrap();
        assert_eq!(count_at_lock, 1);
        assert!(locked_until.is_some());

        // Subsequent attempts while locked should not advance counter
        let (count_after, locked_still) =
            repo.record_failed_attempt(user_id, 1, 900).await.unwrap();
        assert_eq!(
            count_after, 1,
            "counter should not advance when account is already locked"
        );
        assert_eq!(
            locked_still, locked_until,
            "locked_until should remain unchanged"
        );
    }

    #[tokio::test]
    async fn record_failed_attempt_on_nonexistent_user_returns_not_found() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let result = repo.record_failed_attempt(UserId::new(99999), 3, 900).await;
        assert!(
            matches!(result, Err(DomainError::NotFound { .. })),
            "expected NotFound, got {result:?}"
        );
    }

    #[tokio::test]
    async fn clear_failed_attempts_resets_counter_and_lockout() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);
        let user_id = create_test_user(&repo, "clear@shop.local").await;

        // Lock the account
        repo.record_failed_attempt(user_id, 1, 900).await.unwrap();

        // Clear — both counter and lockout should reset
        repo.clear_failed_attempts(user_id).await.unwrap();

        let (_, locked_until) = repo
            .find_lockout_state_by_email("clear@shop.local")
            .await
            .unwrap()
            .unwrap();
        assert!(locked_until.is_none(), "lockout should be cleared");

        // Counter should be back at 0
        let pool = repo.db.get_sqlite_connection_pool();
        let row: (i32,) = sqlx::query_as(
            "SELECT failed_login_attempts FROM users WHERE email = 'clear@shop.local'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(row.0, 0);
    }

    #[tokio::test]
    async fn unlock_user_resets_lockout_and_logs_activity() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());
        let user_id = create_test_user(&repo, "unlock@shop.local").await;
        let admin_id = create_test_user(&repo, "admin@shop.local").await;

        // Lock the account
        repo.record_failed_attempt(user_id, 1, 900).await.unwrap();

        // Admin unlock
        repo.unlock_user(user_id, admin_id).await.unwrap();

        let (_, locked_until) = repo
            .find_lockout_state_by_email("unlock@shop.local")
            .await
            .unwrap()
            .unwrap();
        assert!(
            locked_until.is_none(),
            "lockout should be cleared by unlock_user"
        );

        // Activity log should have account_unlocked entry with admin as actor
        let pool = db.get_sqlite_connection_pool();
        let row: (i64, String, String) = sqlx::query_as(
            "SELECT COUNT(*), MAX(entity_id), MAX(actor_id)
             FROM activity_log WHERE action = 'account_unlocked'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(row.0, 1, "should have one account_unlocked activity entry");
        assert_eq!(row.1, user_id.to_string(), "entity should be unlocked user");
        assert_eq!(row.2, admin_id.to_string(), "actor should be admin");
    }

    #[tokio::test]
    async fn clear_failed_attempts_on_nonexistent_user_returns_not_found() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let result = repo.clear_failed_attempts(UserId::new(99999)).await;
        assert!(
            matches!(result, Err(DomainError::NotFound { .. })),
            "expected NotFound, got {result:?}"
        );
    }

    async fn activity_count(db: &DatabaseConnection, entity_type: &str, action: &str) -> i64 {
        let pool = db.get_sqlite_connection_pool();
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM activity_log WHERE entity_type = ?1 AND action = ?2",
        )
        .bind(entity_type)
        .bind(action)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn create_user_with_codes_success_persists_user_codes_and_activity() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());

        let req = CreateUser {
            email: "composite@shop.local".to_string(),
            name: "Composite".to_string(),
            password: "testpassword123".to_string(),
            role_id: RoleId::new(2),
        };
        let (user, codes) = repo.create_user_with_codes(&req, 10).await.unwrap();

        assert_eq!(user.email, "composite@shop.local");
        assert_eq!(codes.len(), 10, "should return exactly 10 plaintext codes");

        let (_, hash) = repo
            .find_by_email_with_hash("composite@shop.local")
            .await
            .unwrap()
            .unwrap();
        assert!(!hash.is_empty(), "password hash must be set");

        let stored = UserEntity::find()
            .filter(entity::Column::Id.eq(user.id.get()))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(
            stored.recovery_code_hash.is_some(),
            "recovery_code_hash must be stored"
        );

        assert_eq!(
            activity_count(&db, "user", "created").await,
            1,
            "should log exactly one 'user.created' activity"
        );
    }

    #[tokio::test]
    async fn create_user_with_codes_rejects_zero_count() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());

        let req = CreateUser {
            email: "zero@shop.local".to_string(),
            name: "Zero".to_string(),
            password: "pw".to_string(),
            role_id: RoleId::new(2),
        };
        let err = repo.create_user_with_codes(&req, 0).await.unwrap_err();
        match err {
            DomainError::Validation { details } => {
                assert!(
                    details.contains_key("recovery_codes"),
                    "details must name the rejected field, got {details:?}"
                );
            }
            other => panic!("expected DomainError::Validation, got {other:?}"),
        }

        // Rollback is implicit: no user row and no activity row.
        let found = repo.find_by_email("zero@shop.local").await.unwrap();
        assert!(found.is_none(), "rejected batch must not persist user");
        assert_eq!(activity_count(&db, "user", "created").await, 0);
    }

    #[tokio::test]
    async fn create_user_with_codes_rejects_count_above_sixteen() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db);

        let req = CreateUser {
            email: "toomany@shop.local".to_string(),
            name: "Too Many".to_string(),
            password: "pw".to_string(),
            role_id: RoleId::new(2),
        };
        let err = repo.create_user_with_codes(&req, 17).await.unwrap_err();
        assert!(
            matches!(err, DomainError::Validation { .. }),
            "expected Validation for count > 16, got {err:?}"
        );
    }

    #[tokio::test]
    async fn bootstrap_admin_with_codes_success_persists_admin_and_activity() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());

        let (user, codes) = repo
            .bootstrap_admin_with_codes("founder@shop.local", "Founder", "initial-pw")
            .await
            .unwrap();

        assert_eq!(user.email, "founder@shop.local");
        assert_eq!(user.role_id, RoleId::ADMIN);
        assert!(user.is_active);
        assert_eq!(codes.len(), 10);

        assert_eq!(
            activity_count(&db, "user", "bootstrap").await,
            1,
            "should log a 'user.bootstrap' activity"
        );
    }

    #[tokio::test]
    async fn bootstrap_admin_with_codes_marks_setup_complete() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());

        repo.bootstrap_admin_with_codes("founder@shop.local", "Founder", "initial-pw")
            .await
            .unwrap();

        let is_complete = mokumo_shop::db::is_setup_complete(&db).await.unwrap();
        assert!(
            is_complete,
            "bootstrap must leave the install in setup_complete=true state, mirroring create_admin_with_setup"
        );
    }

    #[tokio::test]
    async fn bootstrap_admin_with_codes_rejects_when_admin_exists() {
        let (db, _tmp) = test_db().await;
        let repo = SeaOrmUserRepo::new(db.clone());

        repo.bootstrap_admin_with_codes("first@shop.local", "First", "pw")
            .await
            .unwrap();

        let err = repo
            .bootstrap_admin_with_codes("second@shop.local", "Second", "pw")
            .await
            .unwrap_err();
        assert!(
            matches!(err, BootstrapError::AlreadyBootstrapped),
            "second bootstrap must be rejected, got {err:?}"
        );

        // Second call rolled back — no second user, no new activity entry.
        let found = repo.find_by_email("second@shop.local").await.unwrap();
        assert!(found.is_none(), "rejected bootstrap must not persist user");
        assert_eq!(
            activity_count(&db, "user", "bootstrap").await,
            1,
            "only the first bootstrap should have an activity entry"
        );
    }
}
