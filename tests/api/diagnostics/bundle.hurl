# Auth-gated — run after auth/login-success.hurl in a cookie-jar chain.
GET http://{{host}}/api/diagnostics/bundle

HTTP 200
[Asserts]
header "Content-Type" contains "application/zip"
header "Content-Disposition" contains "attachment"
header "Content-Disposition" contains "mokumo-diagnostics"
bytes isNotEmpty
