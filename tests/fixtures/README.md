# Pre-Stage-3 capture fixtures

Regenerate via: `bash scripts/capture-pre-stage3-fixture.sh`
Captured from: `bfae58e` on 2026-04-17

Contents of `pre-stage3.sqlite`:
- 1 admin user (`capture@pre-stage3.local`)
- 1 customer row (`display_name = "Pre-Stage-3 Capture Customer"`)
- activity_log rows for the setup + customer-create actions
- 8 applied migrations in `seaql_migrations` (pre-S2.1 — `kikan_migrations`
  does not exist in the snapshot; the runner bootstraps + backfills it on replay)

Used by:
- `crates/kikan/tests/migration_replay_snapshot.rs` — proves the runner
  backfills `seaql_migrations` into `kikan_migrations` with
  `graft_id = "mokumo"` and does not re-apply any of the pre-Stage-3
  migrations. Any net-new migrations added after the capture (e.g.
  `m20260416_000000_login_lockout`) apply normally on top of the
  backfilled set.
- Future `tests/api/session_continuity.hurl` — proves session cookies
  minted by pre-Stage-3 code remain valid after the platform lift (see
  `session_continuity.env` for the captured cookie + credentials).

## Regeneration

The capture runs the pre-Stage-3 `mokumo-api` binary against a fresh data
directory, drives setup + login + customer-create, then copies the
resulting profile SQLite and session cookie out. Must run on a host
session — container workspaces cannot create git worktrees inside
`/workspace` (see AGENTS.md). See `scripts/capture-pre-stage3-fixture.sh`
for the full reproduction.
