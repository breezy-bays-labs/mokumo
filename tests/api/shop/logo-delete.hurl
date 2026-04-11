# DELETE /api/shop/logo — deletion smoke tests
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

# Step 2: Upload a logo so we have something to delete
POST http://{{host}}/api/shop/logo
[MultipartFormData]
logo: file,tests/api/shop/fixtures/logo-100x100.png; image/png

HTTP 204

---

# Step 3: Delete the logo → 204
DELETE http://{{host}}/api/shop/logo

HTTP 204

---

# Step 4: Subsequent GET returns 404
GET http://{{host}}/api/shop/logo

HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "shop_logo_not_found"

---

# Step 5: Deleting when no logo exists returns 404
DELETE http://{{host}}/api/shop/logo

HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "shop_logo_not_found"
jsonpath "$.message" isString
jsonpath "$.details" == null
