# LAN access preference — authenticated GET/PUT.
# Default (unset) returns false; PUT upserts and returns the new value.

# 1. GET default — unset preference defaults to false.
GET http://{{host}}/api/settings/lan-access

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.enabled" == false

# 2. PUT true — enables LAN access.
PUT http://{{host}}/api/settings/lan-access
Content-Type: application/json
{
    "enabled": true
}

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.enabled" == true

# 3. GET after enable — reads back true.
GET http://{{host}}/api/settings/lan-access

HTTP 200
[Asserts]
jsonpath "$.enabled" == true

# 4. PUT false — disables LAN access (upsert path).
PUT http://{{host}}/api/settings/lan-access
Content-Type: application/json
{
    "enabled": false
}

HTTP 200
[Asserts]
jsonpath "$.enabled" == false

# 5. GET after disable — reads back false.
GET http://{{host}}/api/settings/lan-access

HTTP 200
[Asserts]
jsonpath "$.enabled" == false
