# POST /api/auth/forgot-password
# Contract smoke tests — success response shape + unknown email rejection.
# The 6-digit PIN is NOT in the response (it's placed in a recovery file).
# Full flow (PIN read from file → reset-password) is owned by BDD.

# 1. Known email — 200 with message and recovery_file_path
POST http://{{host}}/api/auth/forgot-password
Content-Type: application/json
{
    "email": "{{admin_email}}"
}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.message" exists
jsonpath "$.recovery_file_path" exists

# 2. Unknown email — 400 validation_error
POST http://{{host}}/api/auth/forgot-password
Content-Type: application/json
{
    "email": "noone@example.com"
}

HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "validation_error"
jsonpath "$.message" exists
