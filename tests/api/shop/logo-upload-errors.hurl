# POST /api/shop/logo — validation error codes.
# Reads admin session via --cookie from the cookie jar populated by
# tests/api/_prelude/admin-login.hurl.

# GIF upload → 422 logo_format_unsupported
POST http://{{host}}/api/shop/logo
[MultipartFormData]
logo: file,tests/api/shop/fixtures/logo.gif; image/gif

HTTP 422
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "logo_format_unsupported"
jsonpath "$.message" isString
jsonpath "$.details" == null


# Oversized upload (2MiB + 1 byte) → 422 logo_too_large
POST http://{{host}}/api/shop/logo
[MultipartFormData]
logo: file,tests/api/shop/fixtures/logo-too-large.bin; application/octet-stream

HTTP 422
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "logo_too_large"
jsonpath "$.message" isString
jsonpath "$.details" == null


# Empty multipart body (no logo field) → 400 missing_field
POST http://{{host}}/api/shop/logo
Content-Type: multipart/form-data; boundary=boundary123

--boundary123--

HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "missing_field"
jsonpath "$.message" isString
jsonpath "$.details" == null
