# GET /api/shop/logo — retrieval smoke tests
# The GET endpoint is public (no authentication required).

# No logo set → 404 shop_logo_not_found (unauthenticated)
GET http://{{host}}/api/shop/logo

HTTP 404
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.code" == "shop_logo_not_found"
jsonpath "$.message" isString
jsonpath "$.details" == null
