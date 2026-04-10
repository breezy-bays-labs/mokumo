GET http://{{host}}/api/diagnostics

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
header "Cache-Control" contains "no-store"

jsonpath "$.app.name" exists
jsonpath "$.app.version" exists

jsonpath "$.database.production.schema_version" isInteger
jsonpath "$.database.production.wal_mode" isBoolean
jsonpath "$.database.demo.schema_version" isInteger
jsonpath "$.database.demo.wal_mode" isBoolean

jsonpath "$.runtime.uptime_seconds" isInteger
jsonpath "$.runtime.active_profile" matches "^(demo|production)$"
jsonpath "$.runtime.setup_complete" isBoolean
jsonpath "$.runtime.is_first_launch" isBoolean
jsonpath "$.runtime.mdns_active" isBoolean
jsonpath "$.runtime.host" exists
jsonpath "$.runtime.port" isInteger

jsonpath "$.os.family" exists
jsonpath "$.os.arch" exists
