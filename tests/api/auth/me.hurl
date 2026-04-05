# Must be authenticated. Run after login-success.hurl or use --cookie.
# In a CI chain: hurl --cookie-jar /tmp/mokumo.jar --cookie /tmp/mokumo.jar ...
GET http://{{host}}/api/auth/me

HTTP 200
[Asserts]
jsonpath "$.user" exists
jsonpath "$.user.email" exists
jsonpath "$.setup_complete" == true
