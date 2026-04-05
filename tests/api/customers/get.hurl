# Get a specific customer by ID — authenticated.
# Replace {{customer_id}} with a real ID from your seed data or a prior create call.
GET http://{{host}}/api/customers/{{customer_id}}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.id" == "{{customer_id}}"
jsonpath "$.display_name" exists
jsonpath "$.created_at" exists
