# Unauthenticated version endpoint. Returned before login renders so
# the admin SPA can detect engine/UI drift (issue #502).

GET http://{{host}}/api/kikan-version

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.api_version" exists
jsonpath "$.api_version" matches /^\d+\.\d+\.\d+$/
jsonpath "$.engine_version" exists
jsonpath "$.engine_version" matches /^\d+\.\d+\.\d+$/
jsonpath "$.engine_commit" exists
jsonpath "$.engine_commit" != ""
jsonpath "$.schema_versions" exists
jsonpath "$.schema_versions" isObject
