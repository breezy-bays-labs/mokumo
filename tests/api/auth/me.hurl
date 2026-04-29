# GET /api/auth/me — legacy alias for /api/platform/v1/auth/me, preserved
# through M0 for the shop SPA. Wire shape verified against the alias mount
# so the alias-delegation contract cannot drift silently.
#
# Reads the admin session from the cookie jar populated by
# tests/api/_prelude/admin-login.hurl in the prior smoke phase. The canonical
# /me wire shape is also covered by tests/api/platform/auth/me.hurl.
GET http://{{host}}/api/auth/me

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.user" exists
jsonpath "$.user.email" == "{{admin_email}}"
jsonpath "$.setup_complete" exists
