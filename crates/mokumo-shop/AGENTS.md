# mokumo-shop — agent notes

The Mokumo Application — `MokumoApp: kikan::Graft` plus the shop verticals
(customer, shop, sequences, quotes, invoices, kanban, inventory, products,
pricing, financials). Decoration-technique-specific code does NOT belong
here; it goes in `crates/extensions/{technique}/` (see
`ops/decisions/mokumo/adr-mokumo-extensions.md`).

**Layout.** One module per business concern under `src/`, each laid out as
`mod.rs` (re-exports), `domain.rs` (types + repo trait), `repo.rs` (SeaORM
impl), `service.rs`, `handler.rs` (returns `Router<…RouterDeps>`).
Engine glue (`graft.rs`, `lifecycle.rs`, `migrations/`) sits at the crate root.

**Activity logging is part of the mutation contract.** Repo adapters insert
the activity entry inside the same transaction as the mutation via
`kikan::activity::insert_activity_log_raw`. The service layer never
orchestrates logging — atomicity is the adapter's job (rule 12 in root
`CLAUDE.md`).

**Tests.**
- Unit + repo: `cargo test -p mokumo-shop`
- BDD shop suite: `moon run shop:test-bdd` (`tests/features/`)
- BDD HTTP/Axum suite: `moon run shop:test-bdd-api` (`tests/api_features/`)
- Hurl smoke: `moon run shop:smoke` (needs a running server + `hurl` CLI)

**Hurl + error contract.** Every new API endpoint needs a sibling
`tests/api/<domain>/<endpoint>.hurl`. Error responses are
`{"code": "...", "message": "...", "details": null}` — assertions go on
`$.code`, never `$.error`.
