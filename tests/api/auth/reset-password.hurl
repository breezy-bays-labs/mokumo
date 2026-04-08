# POST /api/auth/reset-password
# Contract smoke test — no prior forgot-password request → 400.
# The full success path (forgot-password → read PIN from file → reset-password)
# requires reading a file off the server filesystem and is owned by BDD.

POST http://{{host}}/api/auth/reset-password
Content-Type: application/json
{
    "email": "ghost@example.com",
    "pin": "000000",
    "new_password": "anything"
}

HTTP 400
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "validation_error"
jsonpath "$.message" exists
