# POST /api/auth/recover — vertical recovery-code → password reset.
#
# Happy path is impossible from a smoke harness (we don't have a real
# recovery code at the moment of the call; the seeded production admin's
# 10 codes are captured by `_prelude/setup-wizard.hurl` but consuming one
# here would invalidate the post-setup state for downstream tests).
#
# Per `crates/mokumo-shop/src/auth/recover.rs`, the handler is
# anti-enumeration: rate-limit, password-too-short, AND invalid-code all
# return 400 `validation_error` with the same wire-shape. Asserting that
# rejection contract IS the structural smoke per the bar D split.
POST http://{{host}}/api/auth/recover
Content-Type: application/json
{
    "email": "{{admin_email}}",
    "recovery_code": "0000-0000-0000",
    "new_password": "this-password-is-long-enough-1234"
}

HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "validation_error"
jsonpath "$.message" exists
jsonpath "$.details" == null
