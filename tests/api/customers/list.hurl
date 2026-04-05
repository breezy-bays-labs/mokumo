# List customers — authenticated.
# Hurl sends the session cookie automatically when --cookie-jar is used.
GET http://{{host}}/api/customers

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.items" isCollection
jsonpath "$.total" isInteger
