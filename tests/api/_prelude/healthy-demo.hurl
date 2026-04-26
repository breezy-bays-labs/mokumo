# Smoke-suite prelude — fail fast if the harness didn't seed the demo profile.
#
# `mokumo-server serve` boots into the demo profile by default. With no demo
# data seeded, `demo_install_ok` is false and the boot guard at
# `crates/mokumo-shop/src/auth_handlers/mod.rs` returns 423 with code
# `demo_setup_required` for every data-plane route except `/api/demo/reset`.
# The runtime escape hatch (POST /api/demo/reset) intentionally restarts the
# server, so the seeded state must be in place BEFORE the binary boots.
#
# The smoke harness threads the seed via `MOKUMO_DEMO_SIDECAR=…/demo.db`, which
# kikan's boot-time SidecarRecovery (crates/mokumo-shop/src/graft.rs) copies
# into the demo profile dir and validates. A successful boot publishes
# `install_ok=true` and `status="ok"` on /api/health.
#
# This prelude asserts that contract. If it fails, the rest of the suite is
# guaranteed to fail with cascading 423s — abort here with a clear message
# instead.

GET http://{{host}}/api/health
HTTP 200
[Asserts]
header "Content-Type" contains "application/json"
jsonpath "$.status" == "ok"
jsonpath "$.install_ok" == true
jsonpath "$.storage_ok" == true
jsonpath "$.database" == "ok"
