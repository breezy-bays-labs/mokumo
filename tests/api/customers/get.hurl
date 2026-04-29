# GET /api/customers/:id — fetch a specific customer.
# Self-seeds via POST so the test is profile-agnostic: works against the
# demo profile (which carries seed data) and against a freshly-created
# production profile that has no customer rows yet. Avoids relying on the
# list endpoint having data, which would fail in Phase 2 against the
# post-setup production profile.
# Reads admin session via --cookie from the cookie jar populated by
# tests/api/_prelude/admin-login.hurl.

POST http://{{host}}/api/customers
Content-Type: application/json
{
    "display_name": "Hurl Get-Test Shop"
}

HTTP 201
[Captures]
customer_id: jsonpath "$.id"


GET http://{{host}}/api/customers/{{customer_id}}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.id" == "{{customer_id}}"
jsonpath "$.display_name" == "Hurl Get-Test Shop"
jsonpath "$.created_at" exists
