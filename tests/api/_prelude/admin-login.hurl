# Smoke-suite Phase 2 prelude — capture an admin session via demo auto-login.
#
# `mokumo-server serve` boots into the demo profile by default. Login at
# `/api/auth/login` and `/api/platform/v1/auth/login` always authenticates
# against the production profile (Mokumo's `Graft::auth_profile_kind` is
# `SetupMode::Production` per `crates/mokumo-shop/src/graft.rs`), but the
# demo sidecar seeds `admin@demo.local` into the demo profile DB only — the
# production profile starts empty in the smoke harness, so an explicit POST
# /login as admin@demo.local would 401 `invalid_credentials`.
#
# The demo profile's auto-login middleware (`require_auth_with_demo_auto_login`
# in `crates/mokumo-shop/src/auth/handlers.rs`) bypasses the production-DB
# lookup: any anonymous request to a route mounted behind it issues a fresh
# `mokumo_session` for the demo admin via `auth_session.login()`. This
# prelude pokes such a route (`GET /api/auth/me`) so the response carries
# the Set-Cookie header that `--cookie-jar` persists to a temp file. Phase 2
# files then load that cookie via `--cookie` and the same session id is
# accepted by every authenticated route — including the canonical
# `/api/platform/v1/auth/me` mount that sits OUTSIDE the auto-login layer.
#
# A future production-profile smoke harness would replace this prelude with
# a real `POST /api/auth/login`. That work is out of scope for mokumo#383
# PR 2.
#
# Uses variables: host, admin_email.

GET http://{{host}}/api/auth/me

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
cookie "mokumo_session" exists
jsonpath "$.user.email" == "{{admin_email}}"
