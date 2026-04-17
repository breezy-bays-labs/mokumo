//! Integration tests for `kikan::auth::SeaOrmUserRepo`.
//!
//! These tests live in `crates/db/tests/` (not in kikan) because they need
//! the full mokumo schema (users, roles seed data, settings, activity_log,
//! `updated_at` triggers) — all of which the `mokumo_db` migrator owns.
//! kikan cannot invoke that migrator without violating I4 DAG direction.
//! When `crates/db` dissolves in S3.1b, these tests move to the garment
//! crate's integration test suite.

use kikan::auth::{CreateUser, RoleId, SeaOrmUserRepo, UserId, UserRepository};
use mokumo_shop::db::initialize_database;
use sea_orm::DatabaseConnection;

async fn test_db() -> (DatabaseConnection, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let db = initialize_database(&url).await.unwrap();
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

    let is_complete = mokumo_db::is_setup_complete(&db).await.unwrap();
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
        kikan::auth::password::verify_password("newpass".to_string(), hash)
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

    let password_a = kikan::auth::password::verify_password("newpass-a".to_string(), hash.clone())
        .await
        .unwrap();
    let password_b = kikan::auth::password::verify_password("newpass-b".to_string(), hash)
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

    for code in &codes[0..3] {
        repo.verify_and_use_recovery_code("remain3@test.local", code, "pass")
            .await
            .unwrap();
    }
    assert_eq!(repo.recovery_codes_remaining(&user.id).await.unwrap(), 7);

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
