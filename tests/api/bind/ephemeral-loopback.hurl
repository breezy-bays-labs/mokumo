# Smoke: ephemeral loopback bind
#
# Verifies the server is reachable when bound to an OS-assigned loopback port.
# Run with: hurl --variable host=127.0.0.1:PORT tests/api/bind/ephemeral-loopback.hurl
#
# In CI, `moon run shop:smoke` uses host=localhost:6565 (mokumo-server fixed port).
# For desktop ephemeral-port verification, override the host variable with the
# actual port reported at startup:
#   hurl --variable host=127.0.0.1:<ephemeral_port> tests/api/bind/ephemeral-loopback.hurl
GET http://{{host}}/api/health

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.status" exists
jsonpath "$.storage_ok" isBoolean
