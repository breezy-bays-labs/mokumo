# PATCH /api/users/{id}/role — happy path: no-op flip on the seeded admin.
#
# Phase 2 (--jobs 1): touches `activity_log` via the user-admin write
# path (CLAUDE.md item 12 — adapter-level invariant). Cookie threaded
# by moon.yml from `tests/api/_prelude/admin-login.hurl`.
#
# Why a no-op flip: there is no public user-creation API and no
# test-only seed mechanism for a non-admin user, so the seeded admin is
# the only subject available. We assign Admin → Admin: the kikan
# service-layer last-admin guard at
# `crates/kikan/src/auth/service.rs:56` skips the demote count check
# when `new_role == current_role`, so this exercises the full handler
# stack (auth gate, path extraction, body parse, service call, repo
# update, activity insert, response render) without breaking the
# downstream Phase 2 cookie-jar admin.
#
# DELETE /api/users/{id} happy-path is deferred — see moon.yml ledger.
#
# Uses variables: host. Auth: --cookie populated by Phase 1.5b prelude.

GET http://{{host}}/api/auth/me

HTTP 200
[Captures]
admin_id: jsonpath "$.user.id"


PATCH http://{{host}}/api/users/{{admin_id}}/role
Content-Type: application/json
{
    "role_id": 1
}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.id" isInteger
jsonpath "$.email" exists
jsonpath "$.role_name" == "Admin"
jsonpath "$.is_active" == true
jsonpath "$.updated_at" matches /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/
