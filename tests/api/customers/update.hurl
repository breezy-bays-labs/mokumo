# PUT /api/customers/{id} — update an existing customer. Self-seeds via
# POST so the file is profile-agnostic and parallel-safe (each parallel
# run captures its own customer_id; the customers table has no unique
# constraint on display_name per
# `crates/mokumo-shop/src/migrations/m20260324_000001_customers_and_activity.rs`).
POST http://{{host}}/api/customers
Content-Type: application/json
{
    "display_name": "Hurl Update-Test Shop"
}

HTTP 201
[Captures]
customer_id: jsonpath "$.id"


PUT http://{{host}}/api/customers/{{customer_id}}
Content-Type: application/json
{
    "display_name": "Hurl Updated Shop",
    "phone": "555-0199"
}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.id" == "{{customer_id}}"
jsonpath "$.display_name" == "Hurl Updated Shop"
jsonpath "$.phone" == "555-0199"
jsonpath "$.updated_at" matches /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/
