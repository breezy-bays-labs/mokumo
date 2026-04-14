GET http://{{host}}/api/health

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.status" exists
jsonpath "$.version" exists
jsonpath "$.install_ok" isBoolean
jsonpath "$.storage_ok" isBoolean
