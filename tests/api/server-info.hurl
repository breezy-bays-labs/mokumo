# GET /api/server-info — public endpoint exposing the bound host/port and
# discovery (mDNS / LAN URL) shape. Thin structural smoke per the bar D
# split: assert wire-shape only; ServerInfoResponse is in
# `crates/kikan-types/src/lib.rs`.
GET http://{{host}}/api/server-info

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.host" exists
jsonpath "$.port" exists
jsonpath "$.mdns_active" exists
jsonpath "$.lan_url" exists
jsonpath "$.ip_url" exists
