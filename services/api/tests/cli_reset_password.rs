use mokumo_api::{cli_reset_password, ensure_data_dirs};

/// Set up a temp database with a user for CLI reset tests.
/// Returns (db_path, tempdir). Hold tempdir alive for the test duration.
async fn setup_db_with_user() -> (std::path::PathBuf, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("cli_test");
    ensure_data_dirs(&data_dir).unwrap();
    let db_path = data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_db::initialize_database(&database_url).await.unwrap();

    let repo = kikan::auth::SeaOrmUserRepo::new(pool.clone());
    use kikan::auth::UserRepository;
    use kikan::auth::{CreateUser, RoleId};
    repo.create(&CreateUser {
        email: "admin@shop.local".into(),
        name: "Admin".into(),
        password: "old-password-123".into(),
        role_id: RoleId::ADMIN,
    })
    .await
    .unwrap();

    (db_path, tmp)
}

#[tokio::test]
async fn reset_password_updates_hash() {
    let (db_path, _tmp) = setup_db_with_user().await;

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let original_hash: String = conn
        .query_row(
            "SELECT password_hash FROM users WHERE email = ?1",
            rusqlite::params!["admin@shop.local"],
            |row| row.get(0),
        )
        .unwrap();

    cli_reset_password(&db_path, "admin@shop.local", "new-password-456").unwrap();

    let new_hash: String = conn
        .query_row(
            "SELECT password_hash FROM users WHERE email = ?1",
            rusqlite::params!["admin@shop.local"],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(original_hash, new_hash);

    password_auth::verify_password("new-password-456", &new_hash).unwrap();
}

#[tokio::test]
async fn reset_password_rejects_unknown_email() {
    let (db_path, _tmp) = setup_db_with_user().await;

    let result = cli_reset_password(&db_path, "nobody@shop.local", "password");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No active user found"));
}

#[tokio::test]
async fn reset_password_rejects_soft_deleted_user() {
    let (db_path, _tmp) = setup_db_with_user().await;

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "UPDATE users SET deleted_at = CURRENT_TIMESTAMP WHERE email = ?1",
        rusqlite::params!["admin@shop.local"],
    )
    .unwrap();

    let result = cli_reset_password(&db_path, "admin@shop.local", "new-password-456");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No active user found"));
}

#[tokio::test]
async fn reset_password_rejects_missing_database() {
    let result = cli_reset_password(
        std::path::Path::new("/nonexistent/path/mokumo.db"),
        "admin@shop.local",
        "password",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Cannot open database"));
}

#[tokio::test]
async fn reset_password_works_with_spaces_in_db_path() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("Application Support");
    ensure_data_dirs(&data_dir).unwrap();
    let db_path = data_dir.join("mokumo.db");
    let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = mokumo_db::initialize_database(&database_url).await.unwrap();

    let repo = kikan::auth::SeaOrmUserRepo::new(pool.clone());
    use kikan::auth::UserRepository;
    use kikan::auth::{CreateUser, RoleId};
    repo.create(&CreateUser {
        email: "admin@shop.local".into(),
        name: "Admin".into(),
        password: "old-password-123".into(),
        role_id: RoleId::ADMIN,
    })
    .await
    .unwrap();

    let result = cli_reset_password(&db_path, "admin@shop.local", "new-password-456");
    assert!(
        result.is_ok(),
        "CLI reset-password must succeed when db_path contains spaces: {:?}",
        result.err()
    );
}
