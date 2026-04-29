# DELETE /api/users/:id — 404 when user does not exist.
# Verifies route is registered and NotFound error maps to 404.
# Reads admin session via --cookie from the cookie jar populated by
# tests/api/_prelude/admin-login.hurl.
DELETE http://{{host}}/api/users/99999

HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "not_found"
jsonpath "$.message" exists
jsonpath "$.details" == null
