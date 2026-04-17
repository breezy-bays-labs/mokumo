# Login rate limiting smoke test.
#
# The in-memory limiter allows 10 attempts per email per 15 minutes.
# After 10 failures, the 11th attempt returns 429 rate_limited.
#
# Uses a unique email that does not exist in the database (so none of
# these trigger the DB lockout counter — they hit the "user not found"
# path and return 401 for each of the first 10 attempts).
#
# On the 11th attempt the in-memory limiter fires before the DB lookup,
# returning 429 with code "rate_limited".
#
# Note: the "rate_limited" 429 fires for ALL 11 requests in a fresh run;
# if the test server is shared across runs, prior requests to this email
# may cause 429 to appear earlier than the 11th attempt. The final assert
# only requires 429 on attempt 11.
POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong1" }
HTTP 401
[Asserts]
jsonpath "$.code" == "invalid_credentials"

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong2" }
HTTP 401

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong3" }
HTTP 401

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong4" }
HTTP 401

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong5" }
HTTP 401

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong6" }
HTTP 401

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong7" }
HTTP 401

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong8" }
HTTP 401

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong9" }
HTTP 401

POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong10" }
HTTP 401

# 11th attempt exceeds the in-memory window → 429 rate_limited
POST http://{{host}}/api/auth/login
Content-Type: application/json
{ "email": "ratelimit-smoke-test@smoke.invalid", "password": "wrong11" }
HTTP 429
[Asserts]
header "Content-Type" contains "application/json"
header "Cache-Control" == "no-store"
jsonpath "$.code" == "rate_limited"
jsonpath "$.message" exists
jsonpath "$.details" == null
