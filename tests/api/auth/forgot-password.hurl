# POST /api/auth/forgot-password
# Contract smoke tests — success response shape + unknown email (generic 200).
# The 6-digit PIN is NOT in the response (it's placed in a recovery file).
# Full flow (PIN read from file → reset-password) is owned by BDD.
# Unknown emails return 200 (not 400) to prevent user enumeration via Tunnel access.

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

# 2. Unknown email — same 200 as known email (prevents user enumeration)
POST http://{{host}}/api/auth/forgot-password
Content-Type: application/json
{
    "email": "noone@example.com"
}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.message" exists
