use super::ApiWorld;
use cucumber::{given, then, when};
use mokumo_core::user::traits::UserRepository;
use mokumo_db::user::repo::SeaOrmUserRepo;

// ---- Background ----

#[given("an admin user exists with recovery codes from setup")]
async fn admin_exists_with_recovery_codes(w: &mut ApiWorld) {
    w.ensure_auth().await;
}

// ---- Happy path ----

#[given("the admin is logged in")]
async fn admin_is_logged_in(w: &mut ApiWorld) {
    w.ensure_auth().await;
}

#[when("the admin submits a regeneration request with the correct password")]
async fn submit_regen_with_correct_password(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/account/recovery-codes/regenerate")
            .json(&serde_json::json!({ "password": "testpassword123" }))
            .await,
    );
}

#[then("10 new recovery codes are returned")]
async fn ten_new_codes_returned(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    resp.assert_status(axum::http::StatusCode::OK);
    let body: serde_json::Value = resp.json();
    let codes = body["recovery_codes"]
        .as_array()
        .expect("recovery_codes should be an array");
    assert_eq!(codes.len(), 10, "expected 10 recovery codes");
    // Store the new codes for later use
    w.recovery_codes = codes
        .iter()
        .filter_map(|c| c.as_str().map(String::from))
        .collect();
}

#[then("the codes match the expected format")]
async fn codes_match_expected_format(w: &mut ApiWorld) {
    for code in &w.recovery_codes {
        assert_eq!(code.len(), 9, "code should be 9 chars: {code}");
        assert_eq!(&code[4..5], "-", "code should have hyphen at pos 4: {code}");
        for (i, ch) in code.chars().enumerate() {
            if i == 4 {
                assert_eq!(ch, '-');
            } else {
                assert!(
                    ch.is_ascii_lowercase() || ch.is_ascii_digit(),
                    "unexpected char '{ch}' at position {i} in code {code}"
                );
            }
        }
    }
}

#[then("all previous recovery codes are invalidated")]
async fn previous_codes_invalidated(w: &mut ApiWorld) {
    let original_code = w
        .original_recovery_codes
        .first()
        .expect("no original recovery codes stored from setup")
        .clone();
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let result = repo
        .verify_and_use_recovery_code("admin@test.local", &original_code, "newpass")
        .await
        .unwrap();
    assert!(
        !result,
        "original recovery code should be invalidated after regeneration"
    );
}

// ---- Atomic activity logging ----

#[when("the admin regenerates recovery codes")]
async fn admin_regenerates_codes(w: &mut ApiWorld) {
    let resp = w
        .server
        .post("/api/account/recovery-codes/regenerate")
        .json(&serde_json::json!({ "password": "testpassword123" }))
        .await;
    resp.assert_status(axum::http::StatusCode::OK);
    let body: serde_json::Value = resp.json();
    w.recovery_codes = body["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c.as_str().map(String::from))
        .collect();
}

#[then(expr = "the activity log contains a {string} entry")]
async fn activity_log_contains_entry(w: &mut ApiWorld, action: String) {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM activity_log WHERE action = ?")
        .bind(&action)
        .fetch_one(&w.db_pool)
        .await
        .unwrap();
    assert!(
        row.0 >= 1,
        "expected at least one '{action}' activity log entry, found {}",
        row.0
    );
}

#[then("the activity actor is the authenticated user")]
async fn activity_actor_is_authenticated_user(w: &mut ApiWorld) {
    let row: (String,) = sqlx::query_as(
        "SELECT actor_id FROM activity_log WHERE action = 'recovery_codes_regenerated' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&w.db_pool)
    .await
    .unwrap();
    // actor_id should be a non-empty numeric user id (not "system")
    assert!(
        !row.0.is_empty() && row.0 != "system",
        "actor_id should be a real user id, got: {}",
        row.0
    );
}

// ---- New codes usable ----

#[given("the admin has regenerated recovery codes")]
async fn admin_has_regenerated_codes(w: &mut ApiWorld) {
    admin_regenerates_codes(w).await;
}

#[when("the admin uses one of the new recovery codes for password reset")]
async fn use_new_recovery_code(w: &mut ApiWorld) {
    let code = w.recovery_codes.first().expect("no recovery codes").clone();
    w.response = Some(
        w.server
            .post("/api/auth/recover")
            .json(&serde_json::json!({
                "email": "admin@test.local",
                "recovery_code": code,
                "new_password": "recovered-password-123"
            }))
            .await,
    );
}

#[then("the password reset succeeds")]
async fn password_reset_succeeds(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    resp.assert_status(axum::http::StatusCode::OK);
}

// ---- Old codes rejected ----

#[when("the admin attempts to use an original recovery code")]
async fn attempt_original_recovery_code(w: &mut ApiWorld) {
    let original_code = w
        .original_recovery_codes
        .first()
        .expect("no original recovery codes stored from setup")
        .clone();
    w.response = Some(
        w.server
            .post("/api/auth/recover")
            .json(&serde_json::json!({
                "email": "admin@test.local",
                "recovery_code": original_code,
                "new_password": "should-not-work"
            }))
            .await,
    );
}

#[then("the password reset is rejected")]
async fn password_reset_rejected(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

// ---- Password verification ----

#[when("the admin submits a regeneration request with an incorrect password")]
async fn submit_regen_with_incorrect_password(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/account/recovery-codes/regenerate")
            .json(&serde_json::json!({ "password": "wrong-password" }))
            .await,
    );
}

#[then("the request is rejected with an unauthorized status")]
async fn request_rejected_unauthorized(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[given("the admin has changed their password in another session")]
async fn admin_changed_password(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let user: mokumo_core::user::User = repo
        .find_by_email("admin@test.local")
        .await
        .unwrap()
        .expect("admin should exist");
    repo.update_password(&user.id, "changed-password-456")
        .await
        .unwrap();
}

#[when("the admin submits a regeneration request with the old password")]
async fn submit_regen_with_old_password(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/account/recovery-codes/regenerate")
            .json(&serde_json::json!({ "password": "testpassword123" }))
            .await,
    );
}

// ---- Rate limiting ----

#[when("the admin makes 4 regeneration requests within an hour")]
async fn four_regen_requests(w: &mut ApiWorld) {
    for i in 0..4 {
        let resp = w
            .server
            .post("/api/account/recovery-codes/regenerate")
            .json(&serde_json::json!({ "password": "testpassword123" }))
            .await;
        if i < 3 {
            resp.assert_status(axum::http::StatusCode::OK);
        }
        w.response = Some(resp);
    }
}

#[then("the 4th request is rejected with a rate limit status")]
async fn fourth_request_rate_limited(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    resp.assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);
}

// ---- Authentication ----

#[when("an unauthenticated request is sent to the regeneration endpoint")]
async fn unauthenticated_regen_request(w: &mut ApiWorld) {
    // Create a fresh server without auth to send an unauthenticated request
    // Actually, we just don't call ensure_auth and make the request directly.
    // But ApiWorld always starts unauthenticated — ensure_auth logs in.
    // The Background step calls ensure_auth, so we need a new approach.
    // Since the Background sets up the admin, and this scenario doesn't have
    // "the admin is logged in", the session from ensure_auth is still active.
    // We need to logout first.
    w.server.post("/api/auth/logout").await;

    w.response = Some(
        w.server
            .post("/api/account/recovery-codes/regenerate")
            .json(&serde_json::json!({ "password": "testpassword123" }))
            .await,
    );
}

// "the response is 401 Unauthorized" is already defined in auth_steps.rs

// ---- Code count in /api/auth/me ----

#[when("the admin views their account status")]
async fn admin_views_account_status(w: &mut ApiWorld) {
    w.response = Some(w.server.get("/api/auth/me").await);
}

#[then(expr = "the remaining recovery code count is {int}")]
async fn remaining_code_count(w: &mut ApiWorld, expected: u32) {
    let resp = w.response.as_ref().expect("no response captured");
    resp.assert_status(axum::http::StatusCode::OK);
    let body: serde_json::Value = resp.json();
    let count = body["recovery_codes_remaining"]
        .as_u64()
        .expect("recovery_codes_remaining should be a number");
    assert_eq!(
        count, expected as u64,
        "expected {expected} remaining codes, got {count}"
    );
}

#[given("one recovery code has been used for password reset")]
async fn one_code_used(w: &mut ApiWorld) {
    let code = w.recovery_codes.first().expect("no recovery codes").clone();
    let resp = w
        .server
        .post("/api/auth/recover")
        .json(&serde_json::json!({
            "email": "admin@test.local",
            "recovery_code": code,
            "new_password": "after-recovery-pass"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::OK);

    // Re-login since password changed
    let resp = w
        .server
        .post("/api/auth/login")
        .json(&serde_json::json!({
            "email": "admin@test.local",
            "password": "after-recovery-pass"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::OK);
}

#[given("the admin has used 3 recovery codes")]
async fn admin_has_used_3_codes(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    // Use 3 codes directly via the repo to avoid password changes
    for code in w.recovery_codes[0..3].to_vec() {
        let result = repo
            .verify_and_use_recovery_code("admin@test.local", &code, "testpassword123")
            .await
            .unwrap();
        assert!(result, "recovery code should work: {code}");
    }
    // Re-login with the last password set (testpassword123 from the last recovery)
    let resp = w
        .server
        .post("/api/auth/login")
        .json(&serde_json::json!({
            "email": "admin@test.local",
            "password": "testpassword123"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::OK);
}
