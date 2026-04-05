# /api/auth/me without a session must return 401.
GET http://{{host}}/api/auth/me

HTTP 401
[Asserts]
jsonpath "$.code" == "unauthorized"
jsonpath "$.message" exists
