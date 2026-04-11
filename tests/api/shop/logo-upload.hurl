# POST /api/shop/logo — happy-path upload smoke test
# Requires authentication. Run after setup or with a seeded server.
# Uses variables: host, admin_email, admin_password

# Step 1: Login
POST http://{{host}}/api/auth/login
Content-Type: application/json
{
    "email": "{{admin_email}}",
    "password": "{{admin_password}}"
}

HTTP 200
[Asserts]
cookie "mokumo_session" exists

---

# Step 2: Upload a valid PNG logo
POST http://{{host}}/api/shop/logo
[MultipartFormData]
logo: file,tests/api/shop/fixtures/logo-100x100.png; image/png

HTTP 204

---

# Step 3: Verify setup-status reflects the new logo_url
GET http://{{host}}/api/setup-status

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.logo_url" exists
jsonpath "$.logo_url" contains "/api/shop/logo"
jsonpath "$.logo_url" contains "v="
