# Offline boot smoke — asserts health and server-info respond correctly.
# Does not assert mdns_active (depends on CI network state); that contract
# is proven by the BDD offline_startup.feature scenarios.

GET http://{{host}}/api/health

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.status" == "ok"
jsonpath "$.version" exists
jsonpath "$.database" == "ok"


GET http://{{host}}/api/server-info

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.port" isInteger
jsonpath "$.host" exists
