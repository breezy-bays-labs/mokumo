# Login with wrong password — must return 401, not 500.
POST http://{{host}}/api/auth/login
Content-Type: application/json
{
    "email": "notareal@example.com",
    "password": "wrongpassword"
}

HTTP 401
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "invalid_credentials"
jsonpath "$.message" exists
