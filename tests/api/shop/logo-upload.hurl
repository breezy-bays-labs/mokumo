# POST /api/shop/logo — happy-path upload smoke test.
# Reads admin session via --cookie from the cookie jar populated by
# tests/api/_prelude/admin-login.hurl.

# Step 1: Upload a valid PNG logo
POST http://{{host}}/api/shop/logo
[MultipartFormData]
logo: file,tests/api/shop/fixtures/logo-100x100.png; image/png

HTTP 204


# Step 2: Verify setup-status reflects the new logo_url
GET http://{{host}}/api/setup-status

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.logo_url" exists
jsonpath "$.logo_url" contains "/api/shop/logo"
jsonpath "$.logo_url" contains "v="
