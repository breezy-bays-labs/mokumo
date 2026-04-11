use cucumber::{given, then, when};
use mokumo_core::error::DomainError;
use mokumo_db::shop::{delete_logo, get_logo_info, upsert_logo};

use super::DbWorld;

// ---- Given steps ----

#[given(expr = "a PNG logo exists with epoch {int}")]
async fn png_logo_exists(w: &mut DbWorld, epoch: i64) {
    upsert_logo(&w.db, "png", epoch, "system")
        .await
        .expect("failed to upsert logo");
}

// ---- When steps ----

#[when(expr = "a PNG logo is upserted with epoch {int}")]
async fn upsert_png_logo(w: &mut DbWorld, epoch: i64) {
    match upsert_logo(&w.db, "png", epoch, "system").await {
        Ok(()) => {}
        Err(e) => w.last_logo_error = Some(e),
    }
}

#[when(expr = "a JPEG logo is upserted with epoch {int}")]
async fn upsert_jpeg_logo(w: &mut DbWorld, epoch: i64) {
    match upsert_logo(&w.db, "jpeg", epoch, "system").await {
        Ok(()) => {}
        Err(e) => w.last_logo_error = Some(e),
    }
}

#[when("the logo is deleted")]
async fn delete_the_logo(w: &mut DbWorld) {
    match delete_logo(&w.db, "system").await {
        Ok(()) => {}
        Err(e) => w.last_logo_error = Some(e),
    }
}

// ---- Then steps ----

#[then(expr = "logo_extension should be {string}")]
async fn logo_extension_should_be(w: &mut DbWorld, expected: String) {
    let row = sqlx::query_as::<_, (Option<String>, Option<i64>)>(
        "SELECT logo_extension, logo_epoch FROM shop_settings WHERE id = 1",
    )
    .fetch_optional(&w.pool)
    .await
    .expect("failed to query shop_settings");

    let ext = row
        .and_then(|(ext, _)| ext)
        .expect("expected logo_extension to be set");
    assert_eq!(
        ext, expected,
        "Expected logo_extension={expected}, got {ext}"
    );
}

#[then(expr = "logo_epoch should be {int}")]
async fn logo_epoch_should_be(w: &mut DbWorld, expected: i64) {
    let row = sqlx::query_as::<_, (Option<String>, Option<i64>)>(
        "SELECT logo_extension, logo_epoch FROM shop_settings WHERE id = 1",
    )
    .fetch_optional(&w.pool)
    .await
    .expect("failed to query shop_settings");

    let epoch = row
        .and_then(|(_, ts)| ts)
        .expect("expected logo_epoch to be set");
    assert_eq!(
        epoch, expected,
        "Expected logo_epoch={expected}, got {epoch}"
    );
}

#[then("get_logo_info should return None")]
async fn get_logo_info_returns_none(w: &mut DbWorld) {
    let info = get_logo_info(&w.db)
        .await
        .expect("get_logo_info failed unexpectedly");
    assert!(
        info.is_none(),
        "Expected get_logo_info to return None, got {info:?}"
    );
}

#[then(expr = "the activity log should contain a shop_settings {string} entry")]
async fn activity_log_has_shop_settings_entry(w: &mut DbWorld, action: String) {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT action FROM activity_log WHERE entity_type = 'shop_settings' AND entity_id = '1'",
    )
    .fetch_all(&w.pool)
    .await
    .expect("failed to query activity_log");

    let found = rows.iter().any(|(a,)| a == &action);
    assert!(
        found,
        "Expected 'updated' entry for shop_settings in activity log, found: {:?}",
        rows.iter().map(|(a,)| a.as_str()).collect::<Vec<_>>()
    );
}

#[then(expr = "the activity payload should include action {string}")]
async fn activity_payload_includes_action(w: &mut DbWorld, expected_action: String) {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT payload FROM activity_log WHERE entity_type = 'shop_settings' AND entity_id = '1'",
    )
    .fetch_all(&w.pool)
    .await
    .expect("failed to query activity_log");

    let found = rows.iter().any(|(payload_str,)| {
        serde_json::from_str::<serde_json::Value>(payload_str)
            .ok()
            .and_then(|p| p["action"].as_str().map(String::from))
            .as_deref()
            == Some(&expected_action)
    });

    assert!(
        found,
        "Expected an activity entry with payload.action={expected_action}. Payloads found: {:?}",
        rows.iter().map(|(p,)| p.as_str()).collect::<Vec<_>>()
    );
}

/// Expose last_logo_error for future rollback/failure scenarios.
#[allow(dead_code)]
fn last_logo_error(w: &DbWorld) -> Option<&DomainError> {
    w.last_logo_error.as_ref()
}
