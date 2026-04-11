use super::ApiWorld;
use axum_test::multipart::{MultipartForm, Part};
use cucumber::{given, then, when};

/// Minimal valid 100×100 PNG (white, RGB, no alpha).
/// Generated offline; dimensions and format pass LogoValidator constraints.
const LOGO_PNG: &[u8] = include_bytes!("../../../../tests/api/shop/fixtures/logo-100x100.png");

/// GIF89a magic bytes — fails LogoValidator format check.
const LOGO_GIF: &[u8] = include_bytes!("../../../../tests/api/shop/fixtures/logo.gif");

fn png_form() -> MultipartForm {
    MultipartForm::new().add_part(
        "logo",
        Part::bytes(LOGO_PNG)
            .file_name("logo.png")
            .mime_type("image/png"),
    )
}

fn gif_form() -> MultipartForm {
    MultipartForm::new().add_part(
        "logo",
        Part::bytes(LOGO_GIF)
            .file_name("logo.gif")
            .mime_type("image/gif"),
    )
}

fn oversized_form() -> MultipartForm {
    // 2 MiB + 1 byte — exceeds MAX_BYTES in LogoValidator
    let oversized = vec![0u8; 2 * 1024 * 1024 + 1];
    MultipartForm::new().add_part(
        "logo",
        Part::bytes(oversized)
            .file_name("logo-too-large.bin")
            .mime_type("application/octet-stream"),
    )
}

// ---- Given steps ----

#[given("a logo has been uploaded")]
async fn logo_uploaded(w: &mut ApiWorld) {
    w.ensure_auth().await;
    let resp = w.server.post("/api/shop/logo").multipart(png_form()).await;
    resp.assert_status(axum::http::StatusCode::NO_CONTENT);
}

// ---- When steps ----

#[when("I upload a valid PNG logo")]
async fn upload_png_logo(w: &mut ApiWorld) {
    w.ensure_auth().await;
    w.response = Some(w.server.post("/api/shop/logo").multipart(png_form()).await);
}

#[when("I upload a GIF file as the logo")]
async fn upload_gif_logo(w: &mut ApiWorld) {
    w.ensure_auth().await;
    w.response = Some(w.server.post("/api/shop/logo").multipart(gif_form()).await);
}

#[when("I upload an oversized logo file")]
async fn upload_oversized_logo(w: &mut ApiWorld) {
    w.ensure_auth().await;
    w.response = Some(
        w.server
            .post("/api/shop/logo")
            .multipart(oversized_form())
            .await,
    );
}

#[when("I post multipart with no logo field")]
async fn post_empty_multipart(w: &mut ApiWorld) {
    w.ensure_auth().await;
    // Send multipart with a field named something other than "logo"
    let form = MultipartForm::new().add_text("other_field", "ignored");
    w.response = Some(w.server.post("/api/shop/logo").multipart(form).await);
}

#[when("I delete the logo")]
async fn delete_logo(w: &mut ApiWorld) {
    w.ensure_auth().await;
    w.response = Some(w.server.delete("/api/shop/logo").await);
}

// ---- Then steps ----

#[then(expr = "the logo_url should contain {string}")]
async fn logo_url_contains(w: &mut ApiWorld, expected: String) {
    let resp = w.response.as_ref().expect("no response");
    let json: serde_json::Value = resp.json();
    let logo_url = json["logo_url"]
        .as_str()
        .expect("logo_url should be a non-null string");
    assert!(
        logo_url.contains(&expected),
        "Expected logo_url to contain '{expected}', got '{logo_url}'"
    );
}

#[then(expr = "the response Content-Type should contain {string}")]
async fn response_content_type_contains(w: &mut ApiWorld, expected: String) {
    let resp = w.response.as_ref().expect("no response");
    let header_val = resp.header("content-type");
    let ct = header_val
        .to_str()
        .expect("content-type header should be valid UTF-8");
    assert!(
        ct.contains(&expected),
        "Expected Content-Type to contain '{expected}', got '{ct}'"
    );
}
