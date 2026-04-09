# Routing contract: unknown /api/* paths return JSON 404, never SPA HTML.
# Covers the boundary between API routes and the SPA fallback handler.

# Unknown API path returns 404 JSON
GET http://{{host}}/api/nonexistent
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "not_found"
jsonpath "$.message" exists

# Bare /api returns 404 JSON (not SPA HTML)
GET http://{{host}}/api
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "not_found"
jsonpath "$.message" exists
jsonpath "$.details" == null

# /api/ with trailing slash returns 404 JSON
GET http://{{host}}/api/
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "not_found"
jsonpath "$.message" exists
jsonpath "$.details" == null

# Deeply nested unknown API path returns 404 JSON
GET http://{{host}}/api/v2/customers/list
HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "not_found"
jsonpath "$.message" exists
jsonpath "$.details" == null

# Known API path returns JSON (positive control)
GET http://{{host}}/api/health
HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
