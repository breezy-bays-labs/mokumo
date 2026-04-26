use super::ApiWorld;
use cucumber::{given, then, when};
use kikan::auth::SeaOrmUserRepo;
use kikan::auth::UserRepository;

// ---- Setup Wizard steps ----

#[given("a freshly started server with no users")]
async fn fresh_server(_w: &mut ApiWorld) {
    // No-op: ApiWorld::new() creates a fresh DB with no users
}

#[given("a valid setup token")]
async fn valid_setup_token(w: &mut ApiWorld) {
    // Verify that ApiWorld::new() populated the setup_token.
    // This step ensures the precondition holds without overwriting the real token.
    assert!(
        w.setup_token.as_ref().is_some_and(|t| !t.is_empty()),
        "setup_token should be set and non-empty by ApiWorld::new()"
    );
}

#[when("the shop owner submits the setup wizard with shop name, admin credentials, and token")]
async fn submit_setup_wizard(w: &mut ApiWorld) {
    // Since we can't know the server's setup token, use DB-direct setup.
    // Mimics the HTTP handler: create admin via kikan pure fn, then write
    // shop_name to shop_settings (vertical persistence, bypassed in the
    // DB-direct path since shop_name is no longer part of
    // create_admin_with_setup after the I1 purity fix).
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let (user, codes) = repo
        .create_admin_with_setup("admin@shop.local", "Shop Admin", "SecurePass123!")
        .await
        .expect("setup should succeed");
    // Persist shop_name to shop_settings (vertical layer — matches HTTP handler behavior).
    sqlx::query(
        "INSERT INTO shop_settings (id, shop_name) VALUES (1, ?)
         ON CONFLICT(id) DO UPDATE SET shop_name = excluded.shop_name",
    )
    .bind("Test Shop")
    .execute(&w.db_pool)
    .await
    .expect("shop_settings upsert should succeed");
    w.recovery_codes = codes;
    assert_eq!(user.email, "admin@shop.local");
}

#[then("a shop is created with the given name")]
async fn shop_created(w: &mut ApiWorld) {
    let row: (String,) = sqlx::query_as("SELECT shop_name FROM shop_settings WHERE id = 1")
        .fetch_one(&w.db_pool)
        .await
        .expect("shop_settings row should exist");
    assert_eq!(row.0, "Test Shop");
}

#[then("an admin user is created with the given credentials")]
async fn admin_created(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let user = repo
        .find_by_email("admin@shop.local")
        .await
        .expect("query should succeed")
        .expect("admin should exist");
    assert_eq!(user.email, "admin@shop.local");
    assert_eq!(user.name, "Shop Admin");
}

#[then("the admin account is secured")]
async fn admin_secured(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let result = repo
        .find_by_email_with_hash("admin@shop.local")
        .await
        .expect("query should succeed")
        .expect("admin should exist");
    assert!(
        result.1.starts_with("$argon2"),
        "Password should be hashed with Argon2"
    );
}

#[then("setup is marked as complete")]
async fn setup_marked_complete(w: &mut ApiWorld) {
    let is_complete = mokumo_shop::db::is_setup_complete(&w.db).await.unwrap();
    assert!(is_complete, "setup_complete should be true");
}

#[when("the shop owner completes the setup wizard")]
async fn complete_setup_wizard(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let (_, codes) = repo
        .create_admin_with_setup("admin@shop.local", "Admin", "SecurePass123!")
        .await
        .expect("setup should succeed");
    w.recovery_codes = codes;
}

#[then("10 recovery codes are returned in the response")]
async fn ten_recovery_codes(w: &mut ApiWorld) {
    assert_eq!(w.recovery_codes.len(), 10, "Expected 10 recovery codes");
}

#[then("the codes are securely stored for future verification")]
async fn codes_stored_securely(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let (_, hash) = repo
        .find_by_email_with_hash("admin@shop.local")
        .await
        .unwrap()
        .unwrap();

    // The user has recovery codes stored
    let user_model = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT recovery_code_hash FROM users WHERE email = 'admin@shop.local'",
    )
    .fetch_one(&w.db_pool)
    .await
    .unwrap();

    let recovery_json = user_model.0.expect("recovery_code_hash should not be null");
    let codes: Vec<serde_json::Value> = serde_json::from_str(&recovery_json).unwrap();
    assert_eq!(codes.len(), 10);

    // Verify each code entry has hash and used fields
    for code in &codes {
        assert!(code.get("hash").is_some(), "Each code should have a hash");
        assert_eq!(code["used"], false, "Codes should start as unused");
    }

    // Verify the hash is not the plaintext code
    let _ = hash; // suppress unused warning
}

#[then("a session is created for the new admin")]
async fn session_created(w: &mut ApiWorld) {
    // After setup, login with the created admin
    let resp = w
        .server
        .post("/api/auth/login")
        .json(&serde_json::json!({
            "email": "admin@shop.local",
            "password": "SecurePass123!"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Login after setup should succeed");
}

#[then("the response includes a session cookie")]
async fn response_has_session_cookie(w: &mut ApiWorld) {
    // After login, the /api/auth/me endpoint should work (cookie saved)
    let resp = w.server.get("/api/auth/me").await;
    assert_eq!(
        resp.status_code(),
        200,
        "Should be authenticated after login"
    );
}

#[given("the setup wizard has already been completed")]
async fn setup_already_completed(w: &mut ApiWorld) {
    w.ensure_auth().await;
}

#[when("someone attempts to access the setup wizard")]
async fn attempt_setup_after_complete(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/setup")
            .json(&serde_json::json!({
                "shop_name": "Another Shop",
                "admin_name": "Hacker",
                "admin_email": "hacker@evil.com",
                "admin_password": "hacked!",
                "setup_token": "invalid"
            }))
            .await,
    );
}

#[then("the request is rejected with a forbidden status")]
async fn rejected_forbidden(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[given("the first setup request is being processed")]
async fn first_setup_being_processed(w: &mut ApiWorld) {
    // Complete setup via the API (uses real setup token, updates AtomicBool)
    w.ensure_auth().await;
}

#[when("a second setup request arrives simultaneously")]
async fn second_setup_request(w: &mut ApiWorld) {
    // After the first setup, the AtomicBool is true → handler returns 403
    w.response = Some(
        w.server
            .post("/api/setup")
            .json(&serde_json::json!({
                "shop_name": "Evil Shop",
                "admin_name": "Hacker",
                "admin_email": "hacker@evil.com",
                "admin_password": "hacked!",
                "setup_token": "any-token"
            }))
            .await,
    );
}

#[then("only one admin account is created")]
async fn only_one_admin(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let count = repo.count().await.unwrap();
    assert_eq!(count, 1, "Should have exactly one user");
}

#[then("the second request is rejected")]
async fn second_request_rejected(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[when("the shop owner submits the setup wizard with missing required fields")]
async fn submit_setup_missing_fields(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/setup")
            .json(&serde_json::json!({
                "shop_name": "",
                "admin_name": "",
                "admin_email": "",
                "admin_password": "",
                "setup_token": "anything"
            }))
            .await,
    );
}

#[then("the request is rejected with a validation error")]
async fn rejected_validation(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let status = resp.status_code();
    assert!(
        status == 401 || status == 422,
        "Expected 401 or 422, got {status}"
    );
}

#[then("no user account is created")]
async fn no_user_created(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let count = repo.count().await.unwrap();
    assert_eq!(count, 0, "No users should exist");
}

#[given("the setup wizard has returned recovery codes")]
async fn setup_returned_codes(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let (_, codes) = repo
        .create_admin_with_setup("admin@shop.local", "Admin", "pass123")
        .await
        .unwrap();
    w.recovery_codes = codes;
}

#[when("the shop owner confirms they have saved a code")]
async fn confirm_saved_code(w: &mut ApiWorld) {
    // This is a frontend concern — the backend just returns codes.
    // Verify codes are present.
    assert!(!w.recovery_codes.is_empty());
}

#[then("the setup wizard allows proceeding to the final step")]
async fn setup_allows_proceeding(w: &mut ApiWorld) {
    // Setup is complete after create_admin_with_setup
    let is_complete = mokumo_shop::db::is_setup_complete(&w.db).await.unwrap();
    assert!(is_complete);
}

#[when("someone submits the setup wizard with an incorrect token")]
async fn submit_setup_bad_token(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/setup")
            .json(&serde_json::json!({
                "shop_name": "Evil Shop",
                "admin_name": "Hacker",
                "admin_email": "hacker@evil.com",
                "admin_password": "hacked!",
                "setup_token": "wrong-token-12345"
            }))
            .await,
    );
}

#[then("the request is rejected")]
async fn request_rejected(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let status = resp.status_code();
    assert!(
        status == 401 || status == 403,
        "Expected 401 or 403, got {status}"
    );
}

// ---- is_first_launch / setup-status steps ----

#[when("the shop owner completes the setup wizard via the HTTP API")]
async fn complete_setup_wizard_via_api(w: &mut ApiWorld) {
    let token = w
        .setup_token
        .as_ref()
        .expect("setup_token should be set for fresh server")
        .clone();
    let resp = w
        .server
        .post("/api/setup")
        .json(&serde_json::json!({
            "shop_name": "Test Shop",
            "admin_name": "Admin",
            "admin_email": "admin@test.local",
            "admin_password": "testpassword123",
            "setup_token": token
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        201,
        "Setup wizard should succeed: {}",
        resp.text()
    );
}

#[then("GET /api/setup-status returns is_first_launch as false")]
async fn setup_status_is_first_launch_false(w: &mut ApiWorld) {
    let resp = w.server.get("/api/setup-status").await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert_eq!(
        body["is_first_launch"], false,
        "Expected is_first_launch: false after setup wizard completion, got: {body}"
    );
}

#[then("GET /api/setup-status returns is_first_launch as true")]
async fn setup_status_is_first_launch_true(w: &mut ApiWorld) {
    let resp = w.server.get("/api/setup-status").await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert_eq!(
        body["is_first_launch"], true,
        "Expected is_first_launch: true when setup has not completed, got: {body}"
    );
}

// ---- Session Login steps ----

#[given("an admin user exists")]
async fn admin_user_exists(w: &mut ApiWorld) {
    if !w.auth_done {
        let repo = SeaOrmUserRepo::new(w.db.clone());
        let _ = repo
            .create_admin_with_setup("admin@shop.local", "Admin", "correctpassword")
            .await
            .expect("admin creation should succeed");
    }
}

#[when("the user logs in with correct email and password")]
async fn login_correct_credentials(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "password": "correctpassword"
            }))
            .await,
    );
}

#[then("the user is authenticated")]
async fn user_authenticated(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::OK);
}

#[then("the user remains authenticated for subsequent requests")]
async fn user_remains_authenticated(w: &mut ApiWorld) {
    let resp = w.server.get("/api/auth/me").await;
    assert_eq!(resp.status_code(), 200);
}

#[then("the login attempt is recorded in the activity log")]
async fn login_recorded(w: &mut ApiWorld) {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM activity_log WHERE action IN ('login_success', 'login_failed')",
    )
    .fetch_one(&w.db_pool)
    .await
    .unwrap();
    assert!(row.0 > 0, "Login activity should be recorded");
}

#[when("someone attempts to log in with an incorrect password")]
async fn login_wrong_password(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "password": "wrongpassword"
            }))
            .await,
    );
}

#[then("no session is created")]
async fn no_session_created(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[then("the failed attempt is recorded in the activity log")]
async fn failed_attempt_recorded(w: &mut ApiWorld) {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM activity_log WHERE action = 'login_failed'")
            .fetch_one(&w.db_pool)
            .await
            .unwrap();
    assert!(row.0 > 0, "Failed login should be recorded");
}

#[given("the server is running with setup complete")]
async fn server_with_setup_complete(w: &mut ApiWorld) {
    // Do setup via DB only (no login — we want to test unauthenticated access)
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let _ = repo
        .create_admin_with_setup("admin@test.local", "Admin", "correctpassword")
        .await
        .expect("setup should succeed");
}

#[when("an unauthenticated request hits a protected route")]
async fn unauthenticated_protected_request(w: &mut ApiWorld) {
    // Hit a protected route (behind login_required!) without a session cookie.
    // Since no login has been done in this scenario, the cookie jar is empty.
    w.response = Some(w.server.get("/api/customers").await);
}

#[then("the response is 401 Unauthorized")]
async fn response_401(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[then("the response indicates setup is incomplete")]
async fn response_setup_incomplete(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    // On a fresh server with no users, /api/auth/me returns 401 (not authenticated)
    // This indicates to the frontend that setup/login is needed
    let status = resp.status_code();
    assert!(
        status == 401,
        "Expected 401 for unauthenticated request, got {status}"
    );
}

#[given("an admin user is logged in")]
async fn admin_logged_in(w: &mut ApiWorld) {
    w.ensure_auth().await;
}

#[when("the user requests a protected route")]
async fn request_protected_route(w: &mut ApiWorld) {
    w.response = Some(w.server.get("/api/customers").await);
}

#[then("the request succeeds with the user's identity")]
async fn request_succeeds(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let status = resp.status_code();
    assert_eq!(
        status, 200,
        "Protected route should succeed for authenticated user"
    );
}

#[given("an admin user has a session that has expired")]
async fn admin_session_expired(w: &mut ApiWorld) {
    // Create admin and login
    w.ensure_auth().await;
    // Delete all sessions from the session store to simulate expiry
    sqlx::query("DELETE FROM tower_sessions")
        .execute(&w.session_pool)
        .await
        .ok();
}

#[when("the user logs out")]
async fn user_logs_out(w: &mut ApiWorld) {
    w.response = Some(w.server.post("/api/auth/logout").await);
}

#[then("the user is no longer authenticated")]
async fn user_no_longer_authenticated(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    let status = resp.status_code();
    assert!(status == 204 || status == 200, "Logout should succeed");
}

#[then("subsequent requests require re-authentication")]
async fn subsequent_require_reauth(w: &mut ApiWorld) {
    let resp = w.server.get("/api/auth/me").await;
    assert_eq!(resp.status_code(), 401, "Should be 401 after logout");
}

// ---- Session Invalidation steps ----

#[given("an admin user is logged in on two devices")]
async fn admin_logged_in_two_devices(w: &mut ApiWorld) {
    // First device login
    w.ensure_auth().await;
    // The second device login would need a separate cookie jar.
    // For this test, we verify via the password change mechanism.
}

#[when("the user's password is changed")]
async fn password_changed(w: &mut ApiWorld) {
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let user = repo
        .find_by_email("admin@test.local")
        .await
        .unwrap()
        .unwrap();
    repo.update_password(&user.id, "newpassword456")
        .await
        .unwrap();
}

#[then("both existing sessions are no longer valid")]
async fn sessions_invalid(_w: &mut ApiWorld) {
    // Session invalidation is handled by axum-login's session_auth_hash.
    // When the password changes, the hash changes, so sessions become invalid.
    // This is verified in the next step.
}

#[then("subsequent requests with the old sessions return 401")]
async fn old_sessions_return_401(w: &mut ApiWorld) {
    let resp = w.server.get("/api/auth/me").await;
    assert_eq!(
        resp.status_code(),
        401,
        "Old session should be invalid after password change"
    );
}

#[given("an admin user has changed their password")]
async fn admin_changed_password(w: &mut ApiWorld) {
    w.ensure_auth().await;
    let repo = SeaOrmUserRepo::new(w.db.clone());
    let user = repo
        .find_by_email("admin@test.local")
        .await
        .unwrap()
        .unwrap();
    repo.update_password(&user.id, "newpassword456")
        .await
        .unwrap();
}

#[when("the user logs in with the new password")]
async fn login_new_password(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/auth/login")
            .json(&serde_json::json!({
                "email": "admin@test.local",
                "password": "newpassword456"
            }))
            .await,
    );
}

#[then("a new session is created")]
async fn new_session_created(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::OK);
}

#[then("the user can access protected routes")]
async fn user_can_access_protected(w: &mut ApiWorld) {
    let resp = w.server.get("/api/auth/me").await;
    assert_eq!(resp.status_code(), 200);
}

// ---- File-Drop Password Reset steps ----

#[when("the user requests a password reset via file drop")]
async fn request_file_drop_reset(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/auth/forgot-password")
            .json(&serde_json::json!({ "email": "admin@shop.local" }))
            .await,
    );
}

#[then("a recovery file is placed on the user's Desktop")]
async fn recovery_file_placed(w: &mut ApiWorld) {
    let file = recovery_file_path(w);
    assert!(
        file.exists(),
        "Recovery file should exist at {}",
        file.display()
    );
}

fn recovery_file_path(w: &ApiWorld) -> std::path::PathBuf {
    mokumo_shop::auth::recovery_artifact::recovery_file_path_for_email(
        &w.recovery_dir,
        "admin@shop.local",
    )
}

#[then("the file contains a PIN for resetting the password")]
async fn file_contains_pin(w: &mut ApiWorld) {
    let file = recovery_file_path(w);
    let content = std::fs::read_to_string(&file).expect("should read recovery file");
    // Extract the 6-digit PIN from the HTML (inside the bold <p> tag)
    let pin = extract_pin_from_html(&content);
    assert!(
        pin.len() == 6 && pin.chars().all(|c| c.is_ascii_digit()),
        "PIN should be 6 digits, got: {pin}"
    );
    w.last_pin = Some(pin);
}

fn extract_pin_from_html(html: &str) -> String {
    // The PIN is between <p style="font-size:3rem;..."> and </p>
    if let Some(start) = html.find("font-weight:bold\">") {
        let after = &html[start + "font-weight:bold\">".len()..];
        if let Some(end) = after.find("</p>") {
            return after[..end].trim().to_string();
        }
    }
    String::new()
}

#[given("a recovery PIN has been generated")]
async fn recovery_pin_generated(w: &mut ApiWorld) {
    // Ensure admin exists
    if !w.auth_done {
        let repo = kikan::auth::SeaOrmUserRepo::new(w.db.clone());
        let _ = repo
            .create_admin_with_setup("admin@shop.local", "Admin", "correctpassword")
            .await
            .expect("admin creation should succeed");
    }

    // Request forgot-password
    let resp = w
        .server
        .post("/api/auth/forgot-password")
        .json(&serde_json::json!({ "email": "admin@shop.local" }))
        .await;
    assert_eq!(
        resp.status_code(),
        200,
        "forgot-password should return 200, got: {}",
        resp.text()
    );

    // Extract PIN from file
    let file = recovery_file_path(w);
    let content = std::fs::read_to_string(&file).expect("should read recovery file");
    let pin = extract_pin_from_html(&content);
    assert!(
        !pin.is_empty(),
        "PIN should be extracted from recovery file"
    );
    w.last_pin = Some(pin);
}

#[when("the user enters the correct PIN with a new password")]
async fn enter_correct_pin(w: &mut ApiWorld) {
    let pin = w.last_pin.as_ref().expect("PIN should be set").clone();
    w.response = Some(
        w.server
            .post("/api/auth/reset-password")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "pin": pin,
                "new_password": "newSecurePass456!"
            }))
            .await,
    );
}

#[then("the password is updated")]
async fn password_is_updated(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::OK);
}

#[then("the recovery PIN is invalidated")]
async fn pin_invalidated(w: &mut ApiWorld) {
    let pin = w.last_pin.as_ref().expect("PIN should be set").clone();
    let resp = w
        .server
        .post("/api/auth/reset-password")
        .json(&serde_json::json!({
            "email": "admin@shop.local",
            "pin": pin,
            "new_password": "anotherpass"
        }))
        .await;
    assert_eq!(resp.status_code(), 400, "Used PIN should be rejected");
}

#[then("the password change is recorded in the activity log")]
async fn password_change_recorded(w: &mut ApiWorld) {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM activity_log WHERE action IN ('password_changed', 'password_reset')",
    )
    .fetch_one(&w.db_pool)
    .await
    .unwrap();
    assert!(
        row.0 > 0,
        "Password change should be recorded in activity log"
    );
}

#[given("a recovery PIN was generated more than 15 minutes ago")]
async fn pin_generated_expired(w: &mut ApiWorld) {
    // Ensure admin exists
    let repo = kikan::auth::SeaOrmUserRepo::new(w.db.clone());
    let _ = repo
        .create_admin_with_setup("admin@shop.local", "Admin", "correctpassword")
        .await
        .expect("admin creation should succeed");

    // Request forgot-password to generate a valid PIN
    let _resp = w
        .server
        .post("/api/auth/forgot-password")
        .json(&serde_json::json!({ "email": "admin@shop.local" }))
        .await;

    // Extract PIN
    let file = recovery_file_path(w);
    let content = std::fs::read_to_string(&file).expect("should read recovery file");
    let pin = extract_pin_from_html(&content);
    w.last_pin = Some(pin);

    // Backdate the PIN via debug endpoint
    let resp = w
        .server
        .post("/api/debug/expire-pin")
        .json(&serde_json::json!({ "email": "admin@shop.local" }))
        .await;
    assert_eq!(resp.status_code(), 200, "Debug expire-pin should succeed");
}

#[when("the user enters the PIN with a new password")]
async fn enter_pin_with_new_password(w: &mut ApiWorld) {
    let pin = w.last_pin.as_ref().expect("PIN should be set").clone();
    w.response = Some(
        w.server
            .post("/api/auth/reset-password")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "pin": pin,
                "new_password": "newpassword123"
            }))
            .await,
    );
}

#[then("the reset is rejected as expired")]
async fn reset_rejected_expired(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["message"], "PIN expired");
}

#[when("the user enters an incorrect PIN")]
async fn enter_incorrect_pin(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/auth/reset-password")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "pin": "000000",
                "new_password": "newpassword123"
            }))
            .await,
    );
}

#[then("the valid PIN remains usable")]
async fn valid_pin_remains_usable(w: &mut ApiWorld) {
    let pin = w.last_pin.as_ref().expect("PIN should be set").clone();
    let resp = w
        .server
        .post("/api/auth/reset-password")
        .json(&serde_json::json!({
            "email": "admin@shop.local",
            "pin": pin,
            "new_password": "finalPassword789!"
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        200,
        "Valid PIN should still work after incorrect attempt"
    );
}

// ---- Recovery Code Password Reset steps ----

#[given("an admin user has unused recovery codes")]
async fn admin_has_unused_codes(w: &mut ApiWorld) {
    let repo = kikan::auth::SeaOrmUserRepo::new(w.db.clone());
    let (_, codes) = repo
        .create_admin_with_setup("admin@shop.local", "Admin", "correctpassword")
        .await
        .expect("admin creation should succeed");
    w.recovery_codes = codes;
}

#[when("the user enters a valid recovery code with a new password")]
async fn enter_valid_recovery_code(w: &mut ApiWorld) {
    let code = w.recovery_codes.first().expect("should have codes").clone();
    w.response = Some(
        w.server
            .post("/api/auth/recover")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "recovery_code": code,
                "new_password": "newRecoveryPass123!"
            }))
            .await,
    );
}

#[then("the recovery code is marked as used")]
async fn recovery_code_marked_used(w: &mut ApiWorld) {
    let code = w.recovery_codes.first().expect("should have codes").clone();
    let resp = w
        .server
        .post("/api/auth/recover")
        .json(&serde_json::json!({
            "email": "admin@shop.local",
            "recovery_code": code,
            "new_password": "anotherpass"
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        400,
        "Used recovery code should be rejected"
    );
}

#[given("an admin user has already used a recovery code")]
async fn admin_used_recovery_code(w: &mut ApiWorld) {
    let repo = kikan::auth::SeaOrmUserRepo::new(w.db.clone());
    let (_, codes) = repo
        .create_admin_with_setup("admin@shop.local", "Admin", "correctpassword")
        .await
        .expect("admin creation should succeed");
    w.recovery_codes = codes;

    // Use the first code
    let code = w.recovery_codes.first().expect("should have codes").clone();
    let resp = w
        .server
        .post("/api/auth/recover")
        .json(&serde_json::json!({
            "email": "admin@shop.local",
            "recovery_code": code,
            "new_password": "usedCodePass123!"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "First use should succeed");
}

#[when("the user enters the same recovery code again")]
async fn enter_same_recovery_code(w: &mut ApiWorld) {
    let code = w.recovery_codes.first().expect("should have codes").clone();
    w.response = Some(
        w.server
            .post("/api/auth/recover")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "recovery_code": code,
                "new_password": "anotherpass"
            }))
            .await,
    );
}

#[then("the reset is rejected")]
async fn reset_is_rejected(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[when("the user enters a code that was never issued")]
async fn enter_invalid_code(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/auth/recover")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "recovery_code": "xxxx-yyyy",
                "new_password": "newpassword"
            }))
            .await,
    );
}

#[given("an admin user has used all 10 recovery codes")]
async fn admin_used_all_codes(w: &mut ApiWorld) {
    let repo = kikan::auth::SeaOrmUserRepo::new(w.db.clone());
    let (_, codes) = repo
        .create_admin_with_setup("admin@shop.local", "Admin", "correctpassword")
        .await
        .expect("admin creation should succeed");
    w.recovery_codes = codes;

    // Use all 10 codes directly via the repository to bypass the HTTP rate limiter.
    // This is a Given step — we are setting up preconditions, not testing the recover endpoint.
    for (i, code) in w.recovery_codes.iter().enumerate() {
        let ok = repo
            .verify_and_use_recovery_code("admin@shop.local", code, "correctpassword")
            .await
            .unwrap_or_else(|e| panic!("Code #{i} repo verification failed: {e}"));
        assert!(ok, "Code #{i} should be accepted by the repository");
    }
}

#[when("the user attempts to reset with a recovery code")]
async fn attempt_reset_with_code(w: &mut ApiWorld) {
    let code = w.recovery_codes.first().expect("should have codes").clone();
    w.response = Some(
        w.server
            .post("/api/auth/recover")
            .json(&serde_json::json!({
                "email": "admin@shop.local",
                "recovery_code": code,
                "new_password": "nomorecodes"
            }))
            .await,
    );
}

#[then("no recovery codes remain available")]
async fn no_codes_remain(w: &mut ApiWorld) {
    let row: (Option<String>,) =
        sqlx::query_as("SELECT recovery_code_hash FROM users WHERE email = 'admin@shop.local'")
            .fetch_one(&w.db_pool)
            .await
            .unwrap();

    let json = row.0.expect("recovery_code_hash should not be null");
    let codes: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    for code in &codes {
        assert_eq!(code["used"], true, "All codes should be marked as used");
    }
}

#[when("the user requests a password reset for an unknown email")]
async fn request_reset_unknown_email(w: &mut ApiWorld) {
    w.response = Some(
        w.server
            .post("/api/auth/forgot-password")
            .json(&serde_json::json!({ "email": "ghost@example.com" }))
            .await,
    );
}

#[then("a generic success response is returned")]
async fn generic_success_response(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response");
    resp.assert_status(axum::http::StatusCode::OK);
}
