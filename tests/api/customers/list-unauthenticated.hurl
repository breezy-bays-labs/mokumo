# Customer list without a session must return 401.
# NOTE: This test fails in demo mode (local dev) because require_auth_with_demo_auto_login
# auto-authenticates all requests. It is only meaningful in CI where the server boots
# without demo mode. Excluded from the local smoke run in moon.yml; included in CI.
GET http://{{host}}/api/customers

HTTP 401
[Asserts]
jsonpath "$.code" == "unauthorized"
