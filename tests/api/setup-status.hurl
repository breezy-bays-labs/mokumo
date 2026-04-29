# GET /api/setup-status — public endpoint reporting the bootstrap state
# (active profile, setup_complete flag, shop_name, logo_url). Phase 1
# placement: asserts shape only, never values, so the same file is valid
# under cold-demo (pre-wizard) AND post-wizard production. SetupStatusResponse
# is in `crates/kikan-types/src/setup.rs`.
GET http://{{host}}/api/setup-status

HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.setup_complete" exists
jsonpath "$.is_first_launch" exists
jsonpath "$.production_setup_complete" exists
jsonpath "$.setup_mode" exists
jsonpath "$.shop_name" exists
jsonpath "$.logo_url" exists
