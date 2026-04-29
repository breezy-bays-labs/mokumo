# GET /api/activity — paginated activity-log list. Protected by
# `require_auth_with_demo_auto_login`, so demo auto-login carries the
# call in Phase 1 without requiring a cookie jar. Asserts the
# PaginatedList<ActivityEntryResponse> shape only — `items` MAY be empty
# against a fresh demo profile, so we don't pin a count.
GET http://{{host}}/api/activity

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.items" isCollection
jsonpath "$.total" exists
jsonpath "$.page" exists
jsonpath "$.per_page" exists
