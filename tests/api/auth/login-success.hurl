# Login with valid credentials.
# Uses variables: host, admin_email, admin_password
# Set-Cookie is captured automatically by hurl for subsequent requests in the same file.
POST http://{{host}}/api/auth/login
Content-Type: application/json
{
    "email": "{{admin_email}}",
    "password": "{{admin_password}}"
}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
cookie "id" exists
