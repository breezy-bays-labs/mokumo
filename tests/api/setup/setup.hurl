# POST /api/setup — assert 4xx error contracts pre-setup-completion.
#
# Phase 1, ORDERED BEFORE `_prelude/setup-wizard.hurl` in moon.yml so
# setup is still pending. Once the wizard completes, this endpoint
# returns 403 `forbidden` ("Setup already completed") for every input
# (per `crates/mokumo-shop/src/setup.rs:171` `Conflict` mapping),
# which would mask the input-validation contracts this file exercises.
#
# Two scenarios:
#   1. Empty `shop_name` with otherwise valid fields →
#      422 `validation_error` (handler-level field check at
#      `crates/mokumo-shop/src/setup.rs:64`).
#   2. Bogus `setup_token` (UUID-shaped but never minted) →
#      401 `invalid_token` (token validator returns
#      `ControlPlaneError::PermissionDenied`, mapped at
#      `crates/mokumo-shop/src/setup.rs:174`).
#
# Both calls fail before any DB write, so they leave the pending-setup
# state untouched for the wizard prelude that runs after Phase 1.
#
# Uses variables: host. No auth.

POST http://{{host}}/api/setup
Content-Type: application/json
{
    "shop_name": "",
    "admin_name": "Hurl Setup-Test Admin",
    "admin_email": "hurl-setup-test@example.com",
    "admin_password": "hurl-setup-password-1234567890",
    "setup_token": "00000000-0000-0000-0000-000000000000"
}

HTTP 422
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "validation_error"
jsonpath "$.message" exists
jsonpath "$.details.form" isCollection


POST http://{{host}}/api/setup
Content-Type: application/json
{
    "shop_name": "Hurl Setup-Test Shop",
    "admin_name": "Hurl Setup-Test Admin",
    "admin_email": "hurl-setup-test@example.com",
    "admin_password": "hurl-setup-password-1234567890",
    "setup_token": "00000000-0000-0000-0000-000000000000"
}

HTTP 401
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "invalid_token"
jsonpath "$.message" exists
jsonpath "$.details" == null
