# DELETE /api/shop/logo — deletion smoke tests.
# Reads admin session via --cookie from the cookie jar populated by
# tests/api/_prelude/admin-login.hurl. Uploads then deletes so the test is
# self-contained regardless of prior smoke files.

# Step 1: Upload a logo so we have something to delete
POST http://{{host}}/api/shop/logo
[MultipartFormData]
logo: file,tests/api/shop/fixtures/logo-100x100.png; image/png

HTTP 204


# Step 2: Delete the logo → 204
DELETE http://{{host}}/api/shop/logo

HTTP 204


# Step 3: Subsequent GET returns 404
GET http://{{host}}/api/shop/logo

HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "shop_logo_not_found"


# Step 4: Deleting when no logo exists returns 404
DELETE http://{{host}}/api/shop/logo

HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "shop_logo_not_found"
jsonpath "$.message" isString
jsonpath "$.details" == null
