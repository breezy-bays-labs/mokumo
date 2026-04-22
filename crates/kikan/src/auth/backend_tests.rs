use super::*;
use crate::auth::domain::{CreateUser, RoleId, UserRepository};
use kikan_types::SetupMode;

async fn seed_db() -> (DatabaseConnection, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = mokumo_shop::db::initialize_database(&url).await.unwrap();
    (db, tmp)
}

async fn seed_user(
    db: &DatabaseConnection,
    email: &str,
    password: &str,
) -> crate::auth::domain::User {
    let repo = SeaOrmUserRepo::new(db.clone());
    repo.create(&CreateUser {
        email: email.to_string(),
        name: "Authn Test".to_string(),
        password: password.to_string(),
        role_id: RoleId::new(1),
    })
    .await
    .expect("seed user")
}

fn build_backend(
    demo: DatabaseConnection,
    prod: DatabaseConnection,
    auth_kind: SetupMode,
) -> Backend<SetupMode> {
    let mut map = HashMap::new();
    map.insert(SetupMode::Demo, demo);
    map.insert(SetupMode::Production, prod);
    Backend::new(Arc::new(map), auth_kind)
}

#[tokio::test]
async fn authenticate_returns_user_for_correct_credentials() {
    let (db, _tmp) = seed_db().await;
    seed_user(&db, "admin@shop.local", "correct-horse-battery").await;

    let backend = build_backend(db.clone(), db, SetupMode::Production);
    let result = backend
        .authenticate(Credentials {
            email: "admin@shop.local".to_string(),
            password: "correct-horse-battery".to_string(),
        })
        .await
        .expect("authenticate should succeed");

    let authenticated = result.expect("expected Some(AuthenticatedUser)");
    assert_eq!(authenticated.user.email, "admin@shop.local");
}

#[tokio::test]
async fn authenticate_returns_none_for_unknown_email() {
    let (db, _tmp) = seed_db().await;
    let backend = build_backend(db.clone(), db, SetupMode::Production);

    let result = backend
        .authenticate(Credentials {
            email: "nobody@shop.local".to_string(),
            password: "doesnt-matter".to_string(),
        })
        .await
        .expect("authenticate should not error on missing user");

    assert!(result.is_none(), "unknown email must yield Ok(None)");
}

#[tokio::test]
async fn authenticate_returns_none_for_wrong_password() {
    let (db, _tmp) = seed_db().await;
    seed_user(&db, "admin@shop.local", "correct-horse-battery").await;

    let backend = build_backend(db.clone(), db, SetupMode::Production);
    let result = backend
        .authenticate(Credentials {
            email: "admin@shop.local".to_string(),
            password: "wrong".to_string(),
        })
        .await
        .expect("authenticate should not error on wrong password");

    assert!(result.is_none(), "wrong password must yield Ok(None)");
}

#[tokio::test]
async fn authenticate_returns_none_for_inactive_user() {
    let (db, _tmp) = seed_db().await;
    seed_user(&db, "ghost@shop.local", "secret-value-42").await;
    sqlx::query("UPDATE users SET is_active = 0 WHERE email = 'ghost@shop.local'")
        .execute(db.get_sqlite_connection_pool())
        .await
        .unwrap();

    let backend = build_backend(db.clone(), db, SetupMode::Production);
    let result = backend
        .authenticate(Credentials {
            email: "ghost@shop.local".to_string(),
            password: "secret-value-42".to_string(),
        })
        .await
        .expect("authenticate should not error on inactive user");

    assert!(
        result.is_none(),
        "inactive user must yield Ok(None) before password verify"
    );
}

#[tokio::test]
async fn get_user_returns_none_for_unknown_id() {
    let (db, _tmp) = seed_db().await;
    let backend = build_backend(db.clone(), db, SetupMode::Production);
    let result = backend
        .get_user(&ProfileUserId(SetupMode::Production, 99_999))
        .await
        .expect("get_user should not error on missing id");
    assert!(result.is_none());
}

#[tokio::test]
async fn get_user_dispatches_by_setup_mode() {
    let (prod_db, _prod_tmp) = seed_db().await;
    let (demo_db, _demo_tmp) = seed_db().await;

    let prod_user = seed_user(&prod_db, "prod@shop.local", "pw").await;
    let demo_user = seed_user(&demo_db, "demo@shop.local", "pw").await;

    let backend = build_backend(demo_db, prod_db, SetupMode::Production);

    let found = backend
        .get_user(&ProfileUserId(SetupMode::Production, prod_user.id.get()))
        .await
        .expect("prod lookup")
        .expect("prod user found");
    assert_eq!(found.user.email, "prod@shop.local");

    let found = backend
        .get_user(&ProfileUserId(SetupMode::Demo, demo_user.id.get()))
        .await
        .expect("demo lookup")
        .expect("demo user found");
    assert_eq!(found.user.email, "demo@shop.local");
}

#[test]
fn credentials_deserializes() {
    let json = r#"{"email":"a@b.com","password":"secret"}"#;
    let creds: Credentials = serde_json::from_str(json).unwrap();
    assert_eq!(creds.email, "a@b.com");
    assert_eq!(creds.password, "secret");
}

/// Lock the serde format of ProfileUserId so accidental format changes break CI.
/// axum_login serialises this value into the session store — changing it
/// invalidates all active sessions for live users.
#[test]
fn profile_user_id_roundtrip() {
    let cases = [
        (
            ProfileUserId::<SetupMode>(SetupMode::Demo, 1),
            r#"["demo",1]"#,
        ),
        (
            ProfileUserId::<SetupMode>(SetupMode::Production, 99),
            r#"["production",99]"#,
        ),
    ];

    for (original, expected_json) in cases {
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(
            json, expected_json,
            "serialization format changed for {original:?}"
        );
        let restored: ProfileUserId<SetupMode> = serde_json::from_str(expected_json).unwrap();
        assert_eq!(
            restored, original,
            "deserialization failed for {expected_json}"
        );
    }
}
