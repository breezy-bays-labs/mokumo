# Smoke-suite Phase 2 prelude — capture an admin session via the canonical login.
#
# The HTTP setup wizard (run earlier in the moon.yml smoke task) seeds an
# admin user into the auth profile DB. This prelude POSTs the canonical
# `/api/platform/v1/auth/login` with those credentials so the response
# Set-Cookie is captured by `--cookie-jar` into a temp file. Phase 2 files
# then load that cookie via `--cookie` and the session is accepted by every
# authenticated route — including strict handlers like
# `/api/platform/v1/auth/me` and `/api/users/*` that resolve
# `auth_session.user` via `Backend::get_user(user_id)` against the auth
# profile DB.
#
# The session cookie name is `id` — the tower-sessions default, pinned
# explicitly in `kikan::data_plane::session_layer::SESSION_COOKIE_NAME`.
#
# Uses variables: host, admin_email, admin_password.

POST http://{{host}}/api/platform/v1/auth/login
Content-Type: application/json
{
    "email": "{{admin_email}}",
    "password": "{{admin_password}}"
}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
cookie "id" exists
