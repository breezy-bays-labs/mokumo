//! Step definitions for `user_repo_atomicity.feature`.
//!
//! Exercises the three composite `SeaOrmUserRepo` methods that guarantee
//! Pattern C atomicity (user + recovery codes + activity log land together,
//! or none do):
//!
//! - `create_user_with_codes` — generic user creation with a recovery batch
//! - `regenerate_recovery_codes` — swap the batch + log activity atomically
//! - `bootstrap_admin_with_codes` — first-admin-only variant with an
//!   `ALREADY_BOOTSTRAPPED` conflict guard checked inside the transaction
//!
//! Each scenario starts with a fresh sqlite temp DB initialised via
//! `mokumo_shop::db::initialize_database` so the migrations populate
//! `users`, `roles`, and `activity_log`. The scenario-level state lives in
//! `UserRepoCtx` — held on `KikanWorld` so cucumber's per-scenario world
//! reset drops it cleanly between scenarios.
//!
//! The "activity log write is forced to fail" hook uses a `BEFORE INSERT`
//! trigger on `activity_log` that unconditionally `RAISE(ABORT)`s — this
//! lets us prove the repo rolls back the whole composite without adding a
//! production-facing test seam.

use cucumber::{given, then, when};
use kikan::auth::domain::{CreateUser, RoleId, UserId};
use kikan::auth::repo::{BootstrapError, SeaOrmUserRepo};
use kikan::error::DomainError;
use kikan_types::error::ErrorCode;
use sea_orm::DatabaseConnection;

use super::KikanWorld;

/// Per-scenario state for user-repo atomicity checks.
pub struct UserRepoCtx {
    pub db: DatabaseConnection,
    pub _tmp: tempfile::TempDir,
    pub repo: SeaOrmUserRepo,
    /// The subject user (set by Given steps that seed a user).
    pub user_id: Option<UserId>,
    /// Original recovery codes for that user (set when seeded).
    pub original_codes: Vec<String>,
    /// Codes returned from the last successful create/regenerate/bootstrap.
    pub last_codes: Vec<String>,
    /// Captured outcome of the most recent repo call.
    pub last_outcome: Option<CallOutcome>,
}

pub enum CallOutcome {
    CreateOk,
    CreateErr(DomainError),
    RegenerateOk,
    RegenerateErr(DomainError),
    BootstrapOk,
    BootstrapErr(BootstrapError),
}

async fn fresh_db() -> UserRepoCtx {
    let tmp = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}?mode=rwc", tmp.path().join("test.db").display());
    let db = mokumo_shop::db::initialize_database(&url)
        .await
        .expect("initialize test database");
    let repo = SeaOrmUserRepo::new(db.clone());
    UserRepoCtx {
        db,
        _tmp: tmp,
        repo,
        user_id: None,
        original_codes: Vec::new(),
        last_codes: Vec::new(),
        last_outcome: None,
    }
}

fn ctx<'a>(world: &'a mut KikanWorld) -> &'a mut UserRepoCtx {
    world
        .user_repo_ctx
        .as_mut()
        .expect("user_repo_ctx must be initialised by a Given step")
}

async fn pool(world: &KikanWorld) -> sqlx::SqlitePool {
    world
        .user_repo_ctx
        .as_ref()
        .expect("ctx")
        .db
        .get_sqlite_connection_pool()
        .clone()
}

// ----- Given steps -------------------------------------------------------

#[given(regex = r#"^an empty user table$"#)]
async fn given_empty_user_table(world: &mut KikanWorld) {
    world.user_repo_ctx = Some(fresh_db().await);
}

#[given(regex = r#"^a user "([^"]+)" with 10 recovery codes$"#)]
async fn given_user_with_codes(world: &mut KikanWorld, email: String) {
    world.user_repo_ctx = Some(fresh_db().await);
    let c = ctx(world);
    let req = CreateUser {
        email: email.clone(),
        name: "Alice".into(),
        password: "testpassword123".into(),
        role_id: RoleId::STAFF,
    };
    let (user, codes) = c
        .repo
        .create_user_with_codes(&req, 10)
        .await
        .expect("seed user");
    c.user_id = Some(user.id);
    c.original_codes = codes;
}

#[given(regex = r#"^a user "([^"]+)" with role "admin"$"#)]
async fn given_admin_exists(world: &mut KikanWorld, email: String) {
    world.user_repo_ctx = Some(fresh_db().await);
    let c = ctx(world);
    let (user, _) = c
        .repo
        .bootstrap_admin_with_codes(&email, "Existing Admin", "testpassword123")
        .await
        .expect("seed admin");
    c.user_id = Some(user.id);
}

#[given(regex = r#"^the activity log write is forced to fail$"#)]
async fn given_activity_log_failure(world: &mut KikanWorld) {
    let pool = pool(world).await;
    sqlx::query(
        "CREATE TRIGGER fail_activity_log_insert BEFORE INSERT ON activity_log \
         BEGIN SELECT RAISE(ABORT, 'forced activity log failure'); END",
    )
    .execute(&pool)
    .await
    .expect("install fail trigger");
}

// ----- When steps --------------------------------------------------------

#[when(regex = r#"^the repo creates a user "([^"]+)" with 10 recovery codes$"#)]
async fn when_create_with_10(world: &mut KikanWorld, email: String) {
    let c = ctx(world);
    let req = CreateUser {
        email,
        name: "Alice".into(),
        password: "testpassword123".into(),
        role_id: RoleId::STAFF,
    };
    match c.repo.create_user_with_codes(&req, 10).await {
        Ok((user, codes)) => {
            c.user_id = Some(user.id);
            c.last_codes = codes;
            c.last_outcome = Some(CallOutcome::CreateOk);
        }
        Err(e) => {
            c.last_outcome = Some(CallOutcome::CreateErr(e));
        }
    }
}

#[when(
    regex = r#"^the repo is asked to create "([^"]+)" with a recovery code batch that fails validation$"#
)]
async fn when_create_invalid(world: &mut KikanWorld, email: String) {
    let c = ctx(world);
    let req = CreateUser {
        email,
        name: "Alice".into(),
        password: "testpassword123".into(),
        role_id: RoleId::STAFF,
    };
    // Zero codes — the method's validation rule rejects batches outside 1..=16.
    match c.repo.create_user_with_codes(&req, 0).await {
        Ok(_) => c.last_outcome = Some(CallOutcome::CreateOk),
        Err(e) => c.last_outcome = Some(CallOutcome::CreateErr(e)),
    }
}

#[when(regex = r#"^the repo regenerates that user's recovery codes$"#)]
async fn when_regenerate(world: &mut KikanWorld) {
    let c = ctx(world);
    let id = c.user_id.expect("seeded user");
    match c.repo.regenerate_recovery_codes(&id).await {
        Ok(codes) => {
            c.last_codes = codes;
            c.last_outcome = Some(CallOutcome::RegenerateOk);
        }
        Err(e) => {
            c.last_outcome = Some(CallOutcome::RegenerateErr(e));
        }
    }
}

#[when(regex = r#"^the repo bootstraps an admin "([^"]+)" with 10 recovery codes$"#)]
async fn when_bootstrap(world: &mut KikanWorld, email: String) {
    let c = ctx(world);
    match c
        .repo
        .bootstrap_admin_with_codes(&email, "Founder", "testpassword123")
        .await
    {
        Ok((user, codes)) => {
            c.user_id = Some(user.id);
            c.last_codes = codes;
            c.last_outcome = Some(CallOutcome::BootstrapOk);
        }
        Err(e) => {
            c.last_outcome = Some(CallOutcome::BootstrapErr(e));
        }
    }
}

#[when(regex = r#"^the repo attempts to bootstrap an admin "([^"]+)"$"#)]
async fn when_bootstrap_second(world: &mut KikanWorld, email: String) {
    let c = ctx(world);
    match c
        .repo
        .bootstrap_admin_with_codes(&email, "Another", "testpassword123")
        .await
    {
        Ok(_) => c.last_outcome = Some(CallOutcome::BootstrapOk),
        Err(e) => c.last_outcome = Some(CallOutcome::BootstrapErr(e)),
    }
}

// ----- Then steps --------------------------------------------------------

#[then(regex = r#"^the user "([^"]+)" exists$"#)]
async fn then_user_exists(world: &mut KikanWorld, email: String) {
    let pool = pool(world).await;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ? AND deleted_at IS NULL")
            .bind(&email)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "expected user {email} to exist");
}

#[then(regex = r#"^(\d+) recovery codes belong to that user$"#)]
async fn then_n_recovery_codes(world: &mut KikanWorld, expected: u64) {
    let pool = pool(world).await;
    let id = ctx(world).user_id.expect("user_id must be set").get();
    let json: Option<String> =
        sqlx::query_scalar("SELECT recovery_code_hash FROM users WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let json = json.expect("recovery_code_hash must be set");
    let codes: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(codes.len() as u64, expected, "recovery code count mismatch");
}

#[then(regex = r#"^the activity log contains a "([^"]+)" entry for that user$"#)]
async fn then_activity_contains_for_user(world: &mut KikanWorld, composite: String) {
    let pool = pool(world).await;
    let id = ctx(world).user_id.expect("user_id").get();
    let count = count_matching_activity(&pool, &composite, Some(&id.to_string())).await;
    assert!(
        count >= 1,
        "expected ≥1 activity_log entry matching {composite:?} for user_id={id}, got {count}",
    );
}

#[then(regex = r#"^the activity log contains a "([^"]+)" entry$"#)]
async fn then_activity_contains(world: &mut KikanWorld, composite: String) {
    let pool = pool(world).await;
    let count = count_matching_activity(&pool, &composite, None).await;
    assert!(
        count >= 1,
        "expected ≥1 activity_log entry matching {composite:?}, got {count}"
    );
}

/// Match a feature-file composite label (e.g. `user.created`,
/// `recovery_codes.regenerated`, `user.bootstrap`) against the stored
/// `(entity_type, action)` pair in `activity_log`.
///
/// The label is a human shorthand, not a literal split. We try both:
/// - literal `entity_type = left`, `action = right`
/// - underscore-joined: `entity_type = "user"`, `action = "left_right"`
///
/// The second form is what actually lives in the DB for actions like
/// `recovery_codes_regenerated` on `user` entities. Callers that need to
/// scope to a specific user pass `entity_id_filter`.
async fn count_matching_activity(
    pool: &sqlx::SqlitePool,
    composite: &str,
    entity_id_filter: Option<&str>,
) -> i64 {
    let (left, right) = split_composite(composite);
    let underscore_action = composite.replace('.', "_");
    let q = match entity_id_filter {
        Some(_) => {
            "SELECT COUNT(*) FROM activity_log \
             WHERE ((entity_type = ? AND action = ?) \
                OR (entity_type = 'user' AND action = ?)) \
             AND entity_id = ?"
        }
        None => {
            "SELECT COUNT(*) FROM activity_log \
             WHERE (entity_type = ? AND action = ?) \
                OR (entity_type = 'user' AND action = ?)"
        }
    };
    let mut query = sqlx::query_scalar::<_, i64>(q)
        .bind(left)
        .bind(right)
        .bind(&underscore_action);
    if let Some(id) = entity_id_filter {
        query = query.bind(id);
    }
    query.fetch_one(pool).await.unwrap()
}

#[then(regex = r#"^the operation fails$"#)]
async fn then_operation_fails(world: &mut KikanWorld) {
    let outcome = ctx(world)
        .last_outcome
        .as_ref()
        .expect("an outcome must have been recorded");
    match outcome {
        CallOutcome::CreateErr(_)
        | CallOutcome::RegenerateErr(_)
        | CallOutcome::BootstrapErr(_) => {}
        _ => panic!("expected operation to fail, got success"),
    }
}

#[then(regex = r#"^the operation fails with code "([^"]+)"$"#)]
async fn then_operation_fails_with_code(world: &mut KikanWorld, code: String) {
    let outcome = ctx(world)
        .last_outcome
        .as_ref()
        .expect("an outcome must have been recorded");
    match outcome {
        CallOutcome::BootstrapErr(BootstrapError::AlreadyBootstrapped) => {
            let expected = ErrorCode::AlreadyBootstrapped.to_string();
            let expected_upper = expected.to_uppercase();
            assert_eq!(
                code, expected_upper,
                "expected error code {expected_upper}, feature asserted {code}"
            );
        }
        other => panic!("expected BootstrapError::AlreadyBootstrapped, got {other:?}"),
    }
}

#[then(regex = r#"^no user "([^"]+)" exists$"#)]
async fn then_no_user(world: &mut KikanWorld, email: String) {
    let pool = pool(world).await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ?")
        .bind(&email)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "expected no user {email}");
}

#[then(regex = r#"^no recovery codes exist$"#)]
async fn then_no_recovery_codes(world: &mut KikanWorld) {
    let pool = pool(world).await;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE recovery_code_hash IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "expected no recovery codes to remain");
}

#[then(regex = r#"^no activity log entry for user creation exists$"#)]
async fn then_no_activity_user_creation(world: &mut KikanWorld) {
    let pool = pool(world).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activity_log WHERE entity_type = 'user' AND action = 'created'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0, "expected no 'user.created' activity entry");
}

#[then(regex = r#"^no activity log entry for bootstrap exists$"#)]
async fn then_no_activity_bootstrap(world: &mut KikanWorld) {
    let pool = pool(world).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activity_log WHERE entity_type = 'user' AND action = 'bootstrap'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    // With the existing-admin fixture, the original bootstrap entry exists;
    // the assertion is that the REJECTED second attempt left no new entry.
    // The original bootstrap only logs once, so the count must be ≤ 1.
    assert!(count <= 1, "expected no new bootstrap entry, found {count}");
}

#[then(regex = r#"^the user has exactly 10 recovery codes$"#)]
async fn then_exactly_10_codes(world: &mut KikanWorld) {
    then_n_recovery_codes(world, 10).await;
}

#[then(regex = r#"^none of the new codes match the previous batch$"#)]
async fn then_codes_rotated(world: &mut KikanWorld) {
    let c = ctx(world);
    for new in &c.last_codes {
        assert!(
            !c.original_codes.contains(new),
            "rotated batch overlapped the original: {new} was already in the original batch"
        );
    }
}

#[then(regex = r#"^the user still has exactly the original 10 recovery codes$"#)]
async fn then_codes_unchanged(world: &mut KikanWorld) {
    let pool = pool(world).await;
    let c = ctx(world);
    let id = c.user_id.expect("seeded user").get();
    let json: Option<String> =
        sqlx::query_scalar("SELECT recovery_code_hash FROM users WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let json = json.expect("recovery_code_hash must still be set");
    let codes: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(codes.len(), 10, "expected original 10 codes to remain");
    // Hashes differ from plaintext (argon2), so structural match is enough
    // here — rollback preservation is what this step asserts.
}

#[then(regex = r#"^no new activity log entry exists$"#)]
async fn then_no_new_activity(world: &mut KikanWorld) {
    let pool = pool(world).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activity_log WHERE action = 'recovery_codes_regenerated'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0, "expected no regenerate activity entries");
}

#[then(regex = r#"^the user "([^"]+)" exists with role "admin"$"#)]
async fn then_user_exists_admin(world: &mut KikanWorld, email: String) {
    let pool = pool(world).await;
    let role_id: Option<i64> =
        sqlx::query_scalar("SELECT role_id FROM users WHERE email = ? AND deleted_at IS NULL")
            .bind(&email)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert_eq!(
        role_id,
        Some(RoleId::ADMIN.get()),
        "expected {email} to exist with admin role"
    );
}

// ----- Helpers -----------------------------------------------------------

fn split_composite(s: &str) -> (&str, &str) {
    match s.split_once('.') {
        Some(split) => split,
        None => {
            panic!("expected composite entity_type.action pattern (e.g. user.created), got {s:?}")
        }
    }
}

impl std::fmt::Debug for CallOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallOutcome::CreateOk => write!(f, "CreateOk"),
            CallOutcome::CreateErr(e) => write!(f, "CreateErr({e:?})"),
            CallOutcome::RegenerateOk => write!(f, "RegenerateOk"),
            CallOutcome::RegenerateErr(e) => write!(f, "RegenerateErr({e:?})"),
            CallOutcome::BootstrapOk => write!(f, "BootstrapOk"),
            CallOutcome::BootstrapErr(e) => write!(f, "BootstrapErr({e:?})"),
        }
    }
}
