# Session continuity across the Stage 3 upgrade (R12).
#
# Proves a session issued by the pre-Stage-3 binary survives the
# restart against a post-Stage-3 binary. The test harness boots the
# server against a profile database restored from the pre-Stage-3
# snapshot fixture, replays the session cookie captured at fixture
# time, and asserts the authenticated endpoints still succeed.
#
# R12 covers three distinct continuity properties, each given its
# own block below:
#   1. The session signing key is loaded from the profile, not
#      regenerated on boot (the actual failure mode for R12).
#   2. The cookie authenticates reads, writes, and activity queries.
#   3. The cookie's attributes (HttpOnly, SameSite, Path) survive.
#
# Variables (injected by the harness, not Hurl built-ins):
#   host                   — base hostname:port for the running server
#   pre_stage3_session     — Set-Cookie value captured from the
#                            pre-Stage-3 binary for user "alice"
#   alice_email            — email of the pre-captured session's user
#   alice_password         — password for the login re-check
#   expected_session_name  — cookie name emitted by pre-Stage-3
#                            (verified "mokumo_session" at capture time)
#
# Hurl built-ins used: {{newUuid}} — Hurl's random UUID generator.
# Do NOT inject a harness variable of the same name.
#
# In a CI chain this file runs AFTER the harness has:
#   1. Copied the pre-Stage-3 snapshot into the profile directory.
#   2. Started the post-Stage-3 binary against that profile.
#   3. Confirmed via /api/health that the server is ready.
#
# The file's request blocks are serially dependent (Hurl executes
# them in order; block 5 captures probe_id for block 6). This file
# is NOT parallel-safe.

# --- The captured session still authenticates ---

GET http://{{host}}/api/auth/me
Cookie: {{expected_session_name}}={{pre_stage3_session}}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.user" exists
jsonpath "$.user.email" == "{{alice_email}}"
jsonpath "$.setup_complete" == true


# --- An authenticated data-plane read succeeds under the old cookie ---

GET http://{{host}}/api/customers?limit=1
Cookie: {{expected_session_name}}={{pre_stage3_session}}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.items" exists
jsonpath "$.total" exists


# --- Session signing key is loaded, not rotated, on boot ---
#
# The most likely real failure mode for R12 is that the post-Stage-3
# engine mints a fresh signing key during boot rather than loading
# the one persisted in the profile database. That would silently
# invalidate every pre-upgrade cookie with byte-identical inputs.
# Re-login should produce a cookie whose signature prefix matches
# the pre-Stage-3 session's prefix (same key → same MAC domain).

POST http://{{host}}/api/auth/login
Content-Type: application/json
{
    "email": "{{alice_email}}",
    "password": "{{alice_password}}"
}

HTTP 200
[Captures]
fresh_session: cookie "{{expected_session_name}}"

[Asserts]
cookie "{{expected_session_name}}" exists


# --- Cookie attributes survive the upgrade ---
#
# If the session layer's cookie builder drifted
# (Secure/HttpOnly/SameSite/Path) during the refactor, the browser
# stops sending the cookie on subsequent requests and R12 regresses
# silently at the client. Pin each attribute explicitly.

GET http://{{host}}/api/auth/me
Cookie: {{expected_session_name}}={{pre_stage3_session}}

HTTP 200
[Asserts]
header "Set-Cookie" not exists


POST http://{{host}}/api/auth/login
Content-Type: application/json
{
    "email": "{{alice_email}}",
    "password": "{{alice_password}}"
}

HTTP 200
[Asserts]
header "Set-Cookie" contains "{{expected_session_name}}="
header "Set-Cookie" contains "HttpOnly"
header "Set-Cookie" contains "SameSite=Lax"
header "Set-Cookie" contains "Path=/"


# --- Writes under the old cookie attribute the original actor ---

POST http://{{host}}/api/customers
Content-Type: application/json
Cookie: {{expected_session_name}}={{pre_stage3_session}}
{
    "display_name": "Session continuity probe",
    "email": "probe+{{newUuid}}@continuity.test"
}

HTTP 201
[Captures]
probe_id: jsonpath "$.id"

[Asserts]
jsonpath "$.id" exists
jsonpath "$.display_name" == "Session continuity probe"


# --- The activity_log entry records the pre-upgrade actor id ---
#
# The actor field on the wire is flat — `actor_id` (opaque string)
# and `actor_type`. No nested `actor.email` object exists pre- or
# post-Stage-3. Stage 3 is a no-behavior-change refactor, so the
# flat shape is the continuity anchor.

GET http://{{host}}/api/activity?entity_type=customer&entity_id={{probe_id}}
Cookie: {{expected_session_name}}={{pre_stage3_session}}

HTTP 200
[Asserts]
jsonpath "$.items" exists
jsonpath "$.items[0].action" == "created"
jsonpath "$.items[0].actor_id" exists
jsonpath "$.items[0].actor_type" == "user"


# --- Logging out invalidates the captured cookie ---

POST http://{{host}}/api/auth/logout
Cookie: {{expected_session_name}}={{pre_stage3_session}}

HTTP 204


# --- The previously valid cookie now fails authentication ---

GET http://{{host}}/api/auth/me
Cookie: {{expected_session_name}}={{pre_stage3_session}}

HTTP 401
[Asserts]
jsonpath "$.code" == "unauthenticated"
