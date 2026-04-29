# POST /api/account/recovery-codes/regenerate — admin re-confirms their
# password to mint a fresh batch of 10 recovery codes; old codes are
# invalidated atomically inside `regenerate_recovery_codes`
# (`crates/mokumo-shop/src/auth/handlers.rs`). Phase 2 placement: needs a
# real admin session (cookie jar populated by
# `_prelude/admin-login.hurl`) and shares the same auth-profile DB as
# every other Phase 2 file. State-mutating: lives inside the `--jobs 1`
# serialized set so it cannot race other Phase 2 admin-side writes.
POST http://{{host}}/api/account/recovery-codes/regenerate
Content-Type: application/json
{
    "password": "{{admin_password}}"
}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.recovery_codes" isCollection
jsonpath "$.recovery_codes" count == 10
