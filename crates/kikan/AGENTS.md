# kikan — agent notes

Boundary invariants (I1, I2, I4, I5) are enumerated in `crates/kikan/CLAUDE.md`
and enforced by `scripts/check-i*.sh`. Read that first.

**Layout.** Vertical-slice modules under `src/`: `auth/`, `activity/`, `backup/`,
`boot/`, `control_plane/`, `engine/`, `graft/`, `middleware/`, `migrations/`,
`platform/`, `profile_db/`, `rate_limit/`, `tenancy/`. Each owns its domain
type, repo trait + impl, service, and (where applicable) Axum handler.

**Run kikan-only tests.** `cargo test -p kikan` for fast iteration; the full
backend gate is `moon run shop:test`.

**Composite mutations live inside repo transactions.** When a service-level
operation touches more than one row (e.g. authenticate + bump last-login),
the composite stays inside `repo.rs` under a single SeaORM transaction.
The service layer never opens a transaction — that contract makes activity
logging atomic with the mutation (rule 12 in root `CLAUDE.md`).

**Adding a control-plane handler.** Use `ControlPlaneError` for the handler
signature and let the `From<ControlPlaneError> for AppError` impl render it.
The HTTP and UDS adapters share the same `(ErrorCode, http_status)` tuple —
that equality is pinned by `control_plane_error_variants.feature` and will
fail the BDD suite if you bypass it.
