# DELETE /api/customers/{id} — soft-delete a customer. Self-seeds via
# POST and asserts the response carries a populated `deleted_at`
# timestamp; the row remains in the table (soft delete) and a follow-up
# `restore` PATCH can revive it (see `restore.hurl`).
POST http://{{host}}/api/customers
Content-Type: application/json
{
    "display_name": "Hurl Delete-Test Shop"
}

HTTP 201
[Captures]
customer_id: jsonpath "$.id"


DELETE http://{{host}}/api/customers/{{customer_id}}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.id" == "{{customer_id}}"
jsonpath "$.deleted_at" exists
jsonpath "$.deleted_at" != null
