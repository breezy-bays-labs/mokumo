# Routing contract: wrong HTTP methods on known endpoints return 405 JSON.
# Verifies method_not_allowed_fallback returns structured error responses.

# POST to GET-only /api/health returns 405
POST http://{{host}}/api/health
HTTP 405
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "method_not_allowed"
jsonpath "$.message" exists

# DELETE to GET-only /api/health returns 405
DELETE http://{{host}}/api/health
HTTP 405
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "method_not_allowed"

# PATCH to GET-only /api/health returns 405
PATCH http://{{host}}/api/health
HTTP 405
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "method_not_allowed"

# GET to POST-only /api/auth/login returns 405
GET http://{{host}}/api/auth/login
HTTP 405
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "method_not_allowed"

# PUT to POST-only /api/auth/login returns 405
PUT http://{{host}}/api/auth/login
HTTP 405
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "method_not_allowed"
