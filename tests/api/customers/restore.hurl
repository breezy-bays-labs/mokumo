# PATCH /api/customers/{id}/restore — restore a soft-deleted customer.
# Self-seeds via POST → DELETE → PATCH so the file is parallel-safe and
# profile-agnostic.
POST http://{{host}}/api/customers
Content-Type: application/json
{
    "display_name": "Hurl Restore-Test Shop"
}

HTTP 201
[Captures]
customer_id: jsonpath "$.id"


DELETE http://{{host}}/api/customers/{{customer_id}}

HTTP 200


PATCH http://{{host}}/api/customers/{{customer_id}}/restore

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.id" == "{{customer_id}}"
jsonpath "$.deleted_at" == null
