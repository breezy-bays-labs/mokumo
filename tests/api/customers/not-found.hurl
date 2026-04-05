# Non-existent customer ID must return 404 with a structured error — not 500.
GET http://{{host}}/api/customers/00000000-0000-0000-0000-000000000000

HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "not_found"
jsonpath "$.message" exists
