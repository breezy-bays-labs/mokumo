use mokumo_db::{get_shop_name, initialize_database, is_setup_complete};

#[tokio::test]
async fn fresh_database_reports_setup_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();

    let complete = is_setup_complete(&db).await.unwrap();
    assert!(!complete, "Fresh database should report setup incomplete");
}

#[tokio::test]
async fn completed_setup_is_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();
    let pool = db.get_sqlite_connection_pool();

    sqlx::query("INSERT INTO settings (key, value) VALUES ('setup_complete', 'true')")
        .execute(pool)
        .await
        .unwrap();

    let complete = is_setup_complete(&db).await.unwrap();
    assert!(complete, "Should report setup complete after insert");
}

#[tokio::test]
async fn setup_with_null_value_reports_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();
    let pool = db.get_sqlite_connection_pool();

    sqlx::query("INSERT INTO settings (key, value) VALUES ('setup_complete', NULL)")
        .execute(pool)
        .await
        .unwrap();

    let complete = is_setup_complete(&db).await.unwrap();
    assert!(!complete, "NULL value should report setup incomplete");
}

#[tokio::test]
async fn setup_with_non_true_value_reports_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();
    let pool = db.get_sqlite_connection_pool();

    sqlx::query("INSERT INTO settings (key, value) VALUES ('setup_complete', 'false')")
        .execute(pool)
        .await
        .unwrap();

    let complete = is_setup_complete(&db).await.unwrap();
    assert!(!complete, "Non-'true' value should report setup incomplete");
}

#[tokio::test]
async fn get_shop_name_returns_none_on_fresh_database() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();

    let name = get_shop_name(&db).await.unwrap();
    assert!(name.is_none(), "Fresh database should have no shop name");
}

#[tokio::test]
async fn get_shop_name_returns_stored_value() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();
    let pool = db.get_sqlite_connection_pool();

    sqlx::query("INSERT INTO settings (key, value) VALUES ('shop_name', 'Ink & Thread')")
        .execute(pool)
        .await
        .unwrap();

    let name = get_shop_name(&db).await.unwrap();
    assert_eq!(name.as_deref(), Some("Ink & Thread"));
}

#[tokio::test]
async fn get_shop_name_returns_none_for_null_value() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let db = initialize_database(&url).await.unwrap();
    let pool = db.get_sqlite_connection_pool();

    sqlx::query("INSERT INTO settings (key, value) VALUES ('shop_name', NULL)")
        .execute(pool)
        .await
        .unwrap();

    let name = get_shop_name(&db).await.unwrap();
    assert!(name.is_none(), "NULL shop_name should return None");
}
