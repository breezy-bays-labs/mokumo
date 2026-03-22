use mokumo_db::{initialize_database, is_setup_complete};

#[tokio::test]
async fn fresh_database_reports_setup_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let pool = initialize_database(&url).await.unwrap();

    let complete = is_setup_complete(&pool).await.unwrap();
    assert!(!complete, "Fresh database should report setup incomplete");

    pool.close().await;
}

#[tokio::test]
async fn completed_setup_is_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    let pool = initialize_database(&url).await.unwrap();

    // Insert setup_complete = true
    sqlx::query("INSERT INTO settings (key, value) VALUES ('setup_complete', 'true')")
        .execute(&pool)
        .await
        .unwrap();

    let complete = is_setup_complete(&pool).await.unwrap();
    assert!(complete, "Should report setup complete after insert");

    pool.close().await;
}
