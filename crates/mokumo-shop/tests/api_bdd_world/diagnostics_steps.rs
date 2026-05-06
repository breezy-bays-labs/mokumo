use super::ApiWorld;
use cucumber::then;

/// Navigate a dotted JSON path like "system.total_memory_bytes" through a Value.
fn get_json_path<'v>(value: &'v serde_json::Value, path: &str) -> &'v serde_json::Value {
    let mut current = value;
    for key in path.split('.') {
        current = &current[key];
    }
    current
}

// --- Nested JSON path assertions ---

#[then(expr = "the json path {string} should be a non-negative integer")]
async fn json_path_non_negative_int(w: &mut ApiWorld, path: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    let value = get_json_path(&json, &path);
    value.as_u64().unwrap_or_else(|| {
        panic!("Expected json path '{path}' to be a non-negative integer, got: {value:?}")
    });
}

#[then(expr = "the json path {string} should exist")]
async fn json_path_exists(w: &mut ApiWorld, path: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    // Use serde's pointer() to distinguish "field present (possibly null)"
    // from "field absent." `Value::index` would conflate both as Null.
    // The contract is structural — the field must appear in the response,
    // even when its `Option<T>` value is None (as `build_commit` is in
    // builds without `VERGEN_GIT_SHA`).
    let pointer = format!("/{}", path.replace('.', "/"));
    assert!(
        json.pointer(&pointer).is_some(),
        "Expected json path '{path}' to be present in response: {json}"
    );
}

// --- Boolean JSON path assertion ---

#[then(expr = "the json path {string} should be a boolean")]
async fn json_path_is_boolean(w: &mut ApiWorld, path: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    let value = get_json_path(&json, &path);
    assert!(
        value.is_boolean(),
        "Expected json path '{path}' to be a boolean, got: {value:?}"
    );
}

#[then(expr = "the json path {string} should be null")]
async fn json_path_is_null(w: &mut ApiWorld, path: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    let value = get_json_path(&json, &path);
    assert!(
        value.is_null(),
        "Expected json path '{path}' to be null, got: {value:?}"
    );
}

#[then(expr = "the json path {string} should not be empty")]
async fn json_path_not_empty(w: &mut ApiWorld, path: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let json: serde_json::Value = resp.json();
    let value = get_json_path(&json, &path);
    assert!(
        !value.is_null() && value.as_str().map(|s| !s.is_empty()).unwrap_or(true),
        "Expected json path '{path}' to not be empty, got: {value:?}"
    );
}

// --- Content-type and header assertions for bundle ---

#[then(expr = "the response content type should contain {string}")]
async fn response_content_type_contains(w: &mut ApiWorld, expected: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let header_val = resp.header("content-type");
    let ct = header_val
        .to_str()
        .expect("content-type header is not valid UTF-8");
    assert!(
        ct.contains(&expected),
        "Expected Content-Type to contain '{expected}', got '{ct}'"
    );
}

#[then(expr = "the response should have header {string} containing {string}")]
async fn response_header_contains(w: &mut ApiWorld, header: String, expected: String) {
    let resp = w.response.as_ref().expect("no response captured");
    let header_val = resp.header(&header);
    let actual = header_val
        .to_str()
        .expect("header value is not valid UTF-8");
    assert!(
        actual.contains(&expected),
        "Expected header '{header}' to contain '{expected}', got '{actual}'"
    );
}

#[then("the response body should not be empty")]
async fn response_body_not_empty(w: &mut ApiWorld) {
    let resp = w.response.as_ref().expect("no response captured");
    let bytes = resp.as_bytes();
    assert!(!bytes.is_empty(), "Expected response body to be non-empty");
}
