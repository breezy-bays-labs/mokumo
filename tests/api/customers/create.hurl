# Create a customer — authenticated.
# Only display_name is required; all other fields are optional.
POST http://{{host}}/api/customers
Content-Type: application/json
{
    "display_name": "Hurl Test Shop",
    "email": "hurl-test@example.com",
    "phone": "555-0100"
}

HTTP 201
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.id" exists
jsonpath "$.display_name" == "Hurl Test Shop"
jsonpath "$.email" == "hurl-test@example.com"
jsonpath "$.created_at" exists
jsonpath "$.updated_at" exists
jsonpath "$.deleted_at" == null
