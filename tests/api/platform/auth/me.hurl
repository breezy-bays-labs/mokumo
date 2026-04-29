# GET /api/platform/v1/auth/me — canonical kikan platform endpoint.
# The canonical mount is NOT behind the demo-auto-login middleware, so this
# file relies on an explicit admin session loaded via --cookie from the
# cookie jar populated by tests/api/_prelude/admin-login.hurl.
GET http://{{host}}/api/platform/v1/auth/me

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.user" exists
jsonpath "$.user.email" == "{{admin_email}}"
jsonpath "$.setup_complete" exists
