use cucumber::{given, then, when};

use super::DbWorld;

#[given("a demo database with the admin account seeded and password set")]
async fn given_seeded_admin(w: &mut DbWorld) {
    sqlx::query(
        "INSERT INTO users (email, name, password_hash, role_id, is_active) VALUES (?, ?, ?, 1, 1)",
    )
    .bind("admin@demo.local")
    .bind("Demo Admin")
    .bind("$argon2id$hashed_password")
    .execute(&w.pool)
    .await
    .expect("failed to insert admin user");
}

#[given("a demo database with no admin account")]
async fn given_no_admin(_w: &mut DbWorld) {
    // Default world init creates an empty database — no action needed.
}

#[given("a demo database with an admin account but no password hash stored")]
async fn given_admin_null_password(w: &mut DbWorld) {
    // password_hash column is NOT NULL in the schema, so we insert an empty string
    // to represent the "no password stored" case detected at the application level.
    sqlx::query(
        "INSERT INTO users (email, name, password_hash, role_id, is_active) VALUES (?, ?, '', 1, 1)",
    )
    .bind("admin@demo.local")
    .bind("Demo Admin")
    .execute(&w.pool)
    .await
    .expect("failed to insert admin user with empty password_hash");
}

#[given("a demo database with an admin account and an empty password hash")]
async fn given_admin_empty_password(w: &mut DbWorld) {
    sqlx::query(
        "INSERT INTO users (email, name, password_hash, role_id, is_active) VALUES (?, ?, '', 1, 1)",
    )
    .bind("admin@demo.local")
    .bind("Demo Admin")
    .execute(&w.pool)
    .await
    .expect("failed to insert admin user with empty password_hash");
}

#[when("the installation is validated against that database")]
async fn when_validate(w: &mut DbWorld) {
    w.last_validation_result = Some(mokumo_db::validate_installation(&w.db).await);
}

#[then("the validation passes")]
async fn then_passes(w: &mut DbWorld) {
    assert_eq!(
        w.last_validation_result,
        Some(true),
        "Expected validation to pass"
    );
}

#[then("the validation fails")]
async fn then_fails(w: &mut DbWorld) {
    assert_eq!(
        w.last_validation_result,
        Some(false),
        "Expected validation to fail"
    );
}
