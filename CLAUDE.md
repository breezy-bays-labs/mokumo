@AGENTS.md

# Mokumo — CLAUDE.md

Production management software for decorated apparel shops. Full garment lifecycle:
Quote → Artwork Approval → Production → Shipping → Invoice.

**Architecture**: Self-hosted SvelteKit + Rust (Axum) binary. Shops download, run, own their data.

## Commands

All commands go through Moon. Never run raw `cargo`/`pnpm` directly unless debugging a failure.

```bash
moon run web:dev          # SvelteKit dev server
moon run web:build        # Build SvelteKit frontend (adapter-static)
moon run web:test         # Frontend tests (Vitest)
moon run web:check        # SvelteKit type-check (svelte-check)
moon run web:preview      # Preview production build
moon run shop:dev         # Axum backend with auto-reload (depends on web:build)
moon run shop:build       # Build Rust backend (depends on web:build)
moon run shop:test        # Backend tests (cargo test)
moon run shop:lint        # Clippy lints
moon run shop:fmt         # Check Rust formatting (cargo fmt --check)
moon run shop:fmt-write   # Apply Rust formatting (cargo fmt)
moon run shop:gen-types   # Generate TypeScript from Rust structs (ts-rs)
moon run shop:coverage    # Rust coverage report (JSON, used by CI)
moon run shop:coverage-report # Rust coverage report (HTML, local dev)
moon run shop:smoke           # Hurl HTTP smoke tests (requires running server + hurl CLI)
moon run shop:db-prepare  # Prepare SQLx offline cache (CI)
moon run shop:deny        # Supply-chain audit (advisories, licenses, sources)
moon check --all          # Full CI: lint, test, typecheck, build across all projects
```

Underlying tools: `cargo` (Rust), `pnpm` (SvelteKit). Use directly only when diagnosing Moon task failures.

## Session Startup

- **Host sessions**: code-modifying work uses `claude --worktree` for automatic isolation. If not launched with `--worktree`, use the `EnterWorktree` tool to create one before making changes.
- **Container sessions (cmux/Docker)**: the container **is** the worktree — do NOT run `claude --worktree`, `EnterWorktree`, or `git worktree add` inside `/workspace`. Git writes the new worktree's metadata with container-only paths (e.g. `gitdir: /workspace/...`) into the bind-mounted `.git/worktrees/`, the host sees those entries as `prunable`, and any host `git worktree prune` wipes them — silently breaking every git-backed tool (`moon`, `lefthook`, `gh`) in whichever container was using that metadata. Parallelism inside a container uses sub-agents that share the same `/workspace`; for a genuinely separate workspace, stop and spin up a second host-created worktree in its own container.
- **Never push to main directly** — always branch + PR
- **Commit+push after every logical chunk** — never leave work local-only
- **Run `moon run shop:deny` after touching Cargo.toml or Cargo.lock** — catches advisory, license, and supply-chain issues before CI
- **Update CHANGELOG.md** — add user-facing changes (`feat`, `fix`, `perf`) to the `## Unreleased` section in each PR
- **New API endpoints require a `.hurl` file** — add `tests/api/<domain>/<endpoint>.hurl` in the same PR. Error shape is `{"code": "...", "message": "...", "details": null}` — assert on `$.code`, not `$.error`
- Read-only sessions do not need a worktree

## Tech Stack

| Layer | Technology | Purpose |
|-------|-----------|---------|
| Desktop | Tauri v2 | Native window, embeds Axum server + SvelteKit SPA |
| Frontend | SvelteKit (Svelte 5 runes) + Tailwind v4 + shadcn-svelte | UI, static SPA via adapter-static |
| Backend | Rust (Axum) | API server, binary distribution |
| Database | SQLite (embedded, per-shop) | Zero infrastructure, shop owns the file |
| ORM | SeaORM 2.0 RC (pinned `=2.0.0-rc.38`) | Entity CRUD, migrations, schema management |
| Raw queries | SQLx (compile-time checked) | Complex/reporting queries verified against schema |
| Type sharing | ts-rs crate | Rust structs auto-generate TypeScript interfaces |
| Monorepo | Moon | Polyglot orchestration (Rust + Node) |
| LAN discovery | mDNS (mdns-sd crate) | `{shop}.local` hostname on local network |
| Distribution | Single binary (rust-embed) | SvelteKit SPA embedded in Axum binary |
| Public access | Cloudflare Tunnel | HTTPS without port forwarding |
| Mobile | PWA | Browser-installed, offline-capable |
| Payments | Stripe Connect | Rev-share auto-split or flat monthly |
| Icons | @lucide/svelte | Consistent with design system |

## Project Structure

```
mokumo/
├── .moon/                    # Moon workspace config
├── Cargo.toml                # Rust workspace root
├── apps/
│   ├── mokumo-desktop/       # Tauri v2 desktop binary (kikan + kikan-tauri + mokumo-shop)
│   ├── mokumo-server/        # Headless binary (kikan + kikan-socket + mokumo-shop; zero Tauri)
│   └── web/                  # SvelteKit frontend (adapter-static)
├── crates/
│   ├── kikan/                # Engine — tenancy, migrations, auth, activity, backup,
│   │                          #   platform handlers (diagnostics, demo, backup-status);
│   │                          #   zero vertical-domain knowledge (invariant I1)
│   ├── kikan-events/         # Event-bus SubGraft
│   ├── kikan-mail/           # Mailer SubGraft (SMTP via lettre, CapturingMailer for tests)
│   ├── kikan-scheduler/      # Job scheduler SubGraft (apalis + immediate)
│   ├── kikan-socket/         # Unix domain socket listener primitives
│   ├── kikan-tauri/          # Tauri-shell-specific helpers (ephemeral-port binding)
│   ├── kikan-cli/            # Admin CLI library — clap subcommands + UDS HTTP client
│   │                          #   (subcommand-dispatched by mokumo-server, garage Pattern 3)
│   ├── mokumo-shop/          # Mokumo Application — shop domain + extension API
│   │                          #   (customer, shop, sequences, quotes, invoices, kanban, products,
│   │                          #    generic inventory, cost+markup pricing, migrations)
│   └── extensions/           # Future: crates/extensions/mokumo-{screen-printing,embroidery,dtf,dtg}/
│                              #   introduced one per M4-M8 vertical milestone
└── tools/
    └── license-server/       # Tiny license validation function
```

## Architecture

Mokumo is a kikan-grafted application. Three architectural boundaries:

1. **Engine / Application** (`kikan` ↔ `mokumo-shop`) — see `ops/decisions/mokumo/adr-kikan-engine-vocabulary.md`. The `kikan` crate is the Engine: tenancy, migrations, transport, auth, event bus, mailer, scheduler. `mokumo-shop` is the Application: shop domain, HTTP routes, migrations, business rules. They fuse through the `Graft` trait at compile time.
2. **Control plane / Data plane** — see `ops/decisions/mokumo/adr-control-plane-data-plane-split.md` and `adr-tauri-http-not-ipc.md`. Control plane = admin surface (diagnostics, backup, tenant management, bootstrap). Data plane = business endpoints (customers, quotes, orders). Both are HTTP-backed Axum routes; the control plane subset is served additionally on a Unix domain socket with mode 0600 as a capability-based admin channel.
3. **Application / Extensions** (`mokumo-shop` ↔ `crates/extensions/*`) — see `ops/decisions/mokumo/adr-mokumo-extensions.md`. Decoration techniques (screen printing, embroidery, DTF, DTG) compose into `mokumo-shop` through a typed `ExtensionRegistry` with per-`LineKind` dispatch (Backstage-style). Extensions own their own side tables; they never extend `mokumo-shop` base tables. Per-profile activation lives in the meta DB. Not yet built — M4 work.

### Crate roles

- **`crates/kikan/`** — **Engine.** Tenancy, per-profile migration runner, auth (repo + backend + sessions), activity log writer, backup/restore primitives, platform handlers (diagnostics, backup-status, demo reset, discovery/mDNS), SeaORM pool init, middleware (host allow-list, ProfileDb extractor, session layer), event bus types, `PlatformState`, `Engine<G: Graft>`. **Zero vertical-domain knowledge** (invariant I1).
- **`crates/kikan-{events,mail,scheduler,socket,tauri,cli}/`** — Engine satellites. Each is a single-responsibility adapter or SubGraft contributor.
- **`crates/mokumo-shop/`** — **Application.** Shop domain with extension API surface + `MokumoApp: Graft` impl, lifecycle hooks, data-plane router composition, and the BDD/HTTP integration suite under `tests/api_bdd*`. Neutral to decoration technique — decorator-specific concepts (artwork, gang-sheets, stitch-count math) do NOT live here; they live in `crates/extensions/{technique}/` when each milestone introduces its technique. `mokumo-decor` as an anticipatory intermediate crate is **not** introduced now (see amendment in `adr-workspace-split-kikan.md` and `adr-mokumo-extensions.md` §Alternative B rejected).
- **`apps/mokumo-desktop/`** — Tauri binary composing `kikan` + `kikan-tauri` + `mokumo-shop` + `mokumo-spa` for the desktop delivery shell.
- **`apps/mokumo-server/`** — Headless binary composing `kikan` + `kikan-socket` + `mokumo-shop` + `kikan-cli` for the Linux/container delivery shell. **Zero transitive Tauri dependency** (invariant I3, CI-enforced).

### Load-bearing invariants (see `ops/decisions/mokumo/adr-workspace-split-kikan.md` §I1-I5)

- **I1 — Domain purity.** `crates/kikan/src/` contains no shop-vertical identifiers (`customer`, `garment`, `quote`, `invoice`, `print_job`). Shop language belongs in `mokumo-shop`.
- **I2 — Adapter boundary.** No `tauri::` or `#[tauri::command]` under `crates/kikan/**`. CI-enforced.
- **I3 — Headless zero-Tauri.** `cargo tree -p mokumo-server | grep -E '^tauri(-[a-z-]+)?( |$)'` exits non-zero. CI-enforced.
- **I4 — One-way DAG.** `kikan` depends on nothing in the workspace. Adapter crates depend on `kikan`. `mokumo-shop` depends on `kikan`. Binaries compose `kikan` + `mokumo-shop` (+ `kikan-tauri`/`mokumo-spa` for desktop, `kikan-socket`/`kikan-cli` for server).
- **I5 — Feature gates carry Tauri-reachability.** No Cargo feature anywhere pulls Tauri into `kikan`, `kikan-socket`, `mokumo-shop`, or `mokumo-server`.

### Feature organization (vertical slice pattern)

Within each crate, features organize as vertical slices — a module per business concern contains its types, traits, service, repo, and HTTP handler side by side:

```
crates/mokumo-shop/src/
  customer/
    mod.rs          # re-exports
    domain.rs       # Customer type, CustomerId newtype, CustomerRepository trait
    repo.rs         # SqliteCustomerRepository (SeaORM impl)
    service.rs      # CustomerService business logic
    handler.rs      # customer_router() -> Router<CustomerRouterDeps>
  shop/
    ...

crates/kikan/src/
  auth/
    mod.rs          # re-exports
    domain.rs       # User, Role, UserRepository trait
    repo.rs         # SeaOrmUserRepo (composite methods inside transactions)
    backend.rs      # axum-login Backend
    user.rs         # AuthenticatedUser, ProfileUserId session types
```

**Router contribution pattern**: each module that owns HTTP routes exposes a `…RouterDeps` struct holding ONLY singleton dependencies (e.g., `Arc<dyn ActivityWriter>`, rate-limiters) plus a `…_router() -> Router<RouterDeps>` builder. Per-request state (DB handle, session, authenticated user) is extracted via Axum extractors. `kikan::Engine::build_router` assembles the 5-layer middleware stack and nests the domain routes returned by `Graft::data_plane_routes` under `/api/`. This keeps router deps narrow and lets the same sub-router be mounted into the UDS admin surface when appropriate.

**Import rule**: `kikan` never imports from `mokumo-shop` or any extension. Dependencies flow toward `kikan`, never away from it.

## Coding Standards

1. **Rust newtypes for entity IDs** — `struct CustomerId(uuid::Uuid)`, not bare `String`. Never implement `Deref`/`DerefMut` on newtypes — use `.get()` for inner access, `From`/`Into` for conversion. Keep `sea-orm` and `sqlx` derives out of domain types. `DeriveEntityModel` and `FromRow` belong on infrastructure types only (inside whichever crate owns the repo impl). Domain types live with their business logic; wire types for ts-rs live in `crates/kikan-types/`. See `ops/decisions/mokumo/adr-entity-type-placement.md`.
2. **Financial arithmetic in Rust** — money types with fixed-point or integer-cents representation. Never floating-point for prices, totals, or tax.
3. **Hybrid ORM + raw SQL** — SeaORM for entity CRUD operations, `sqlx::query!()` / `sqlx::query_as!()` for complex joins, reporting, and aggregate queries. Never string-concatenated SQL in either approach.
4. **Svelte 5 runes only** — `$state`, `$derived`, `$effect`, `$props`. Never Svelte 4 stores or `export let`.
5. **Axum patterns** — standard Axum server setup, SQLite PRAGMAs (WAL, foreign_keys, busy_timeout), `thiserror` + `IntoResponse` error handling, repository traits with `Send + Sync` bounds. Route builders per module return `Router<SomeRouterDeps>` with singleton deps only; per-request state comes from extractors (see §Architecture).
6. **ts-rs type sharing** — API DTOs live in `crates/kikan-types/` and derive `TS` + `Serialize` for TypeScript generation. SeaORM entity types are infrastructure, not shared. Run `moon run shop:gen-types` to regenerate TypeScript bindings.
7. **Error handling** — two layers: `ControlPlaneError` (narrow, handler-level; in `crates/kikan/src/error/`) for admin surface handler signatures, and `AppError` (wider; in `crates/kikan/src/app_error.rs`) for HTTP transport rendering. HTTP adapters convert via `From<ControlPlaneError> for AppError`. UDS adapters render `ControlPlaneError` directly. Both paths produce the same `(ErrorCode, http_status)` tuple — that equality is pinned by `control_plane_error_variants.feature`.
8. **No raw SQL injection** — parameterized queries only.
9. **URL state** — filters, search, pagination in URL query params. Svelte `$state` for ephemeral UI state only.
10. **Repository traits** — `async fn` in traits (Rust 1.75+, no `async-trait` crate **except** where object-safety is required — see `ActivityWriter`). Traits live with the domain they serve; impls live next to them. Bounds: `Send + Sync` only.
11. **SQLite `updated_at` triggers** — every mutable table gets an `AFTER UPDATE` trigger in its migration.
12. **Activity logging is part of the mutation contract, enforced by the adapter.** Entity repository adapters insert activity log entries within the same transaction as the mutation using `kikan::activity::insert_activity_log_raw(tx, entry)`. The service layer does not orchestrate logging — atomicity is guaranteed by the adapter. Every entity vertical (customer, shop, quote, invoice, ...) follows this pattern. `actor_id` is `TEXT NOT NULL DEFAULT 'system'` — no FK to users; transport-native actor tags are the contract (platform callers pass a UUID string, system-initiated actions use `'system'`).
13. **No sealed traits on internal crates** — crate boundaries provide sufficient encapsulation. Sealing blocks test doubles.
14. **SeaORM entity placement** — entities with `DeriveEntityModel` live with their repo impl in whichever crate owns the data (`mokumo-shop` for shop verticals, `kikan` for platform tables like `users`, `activity_log`, `profile_active_extensions`). Never put `DeriveEntityModel` in `kikan-types` or in a domain-pure module.
15. **SeaORM migrations** — every migration returns `Some(true)` from `use_transaction()` (atomic SQLite migrations). Pre-migration backup is non-negotiable. `updated_at` triggers still required per item 11. Migrations compose through kikan's per-profile DAG runner: `kikan::SelfGraft` contributes platform-owned migrations; the primary `Graft` (mokumo's `MokumoApp`) contributes vertical migrations; SubGrafts (mailer, scheduler) contribute their own.
16. **Pre-implementation boundary checklist** — before writing any conditional, path-matching, or range-checking code, answer four questions: (a) What are the boundary values? (b) What happens *at* each boundary? (c) What is the "almost right" input that should be rejected? (d) How does the caller see a rejected input (error code, status, message)? Each answer should have a corresponding test. See `ops/standards/testing/negative-path.md`.
17. **I4 DAG discipline** — `kikan` depends on nothing in the workspace. `mokumo-shop` depends on `kikan` only (and `kikan-types`, `mokumo-core` transitively). Binaries (`mokumo-desktop`, `mokumo-server`) compose multiple crates. If a change would make kikan depend on mokumo-shop, pause and rethink — the surface probably belongs on kikan's side of the boundary or behind a new trait kikan owns.
18. **Comments describe current reality, never history.** Default to writing no comment. When you do add one, apply the **forward-dating test**: read it as if six months have passed and you have no memory of the PR that added it. If the comment still makes sense, keep it. If it feels archaeological, delete it before committing. Specifically forbidden:
    - PR / issue numbers in code comments (`#512`, `PR 4c`). The commit message and PR description are the audit trail — the comment is not.
    - Lineage narration: "moved from X", "lifted from Y", "migrated from Z", "relocated in …", "previously lived in …", "renamed from …". If the new location is right, nobody needs to know the old one; `git log` does.
    - Stage / phase / wave narrative: "post-Stage-3", "Wave A.2 lifted this", "S2.5 relocated", "V6c deletion sweep", "once Stage 1b lands". Internal milestone names rot the moment the milestone closes.
    - Temporal language: "recently", "newly", "now lives in", "used to live in", "currently", "for now", "today … later". If the comment needs a time qualifier to be true, it will be wrong soon.
    - Speculation about future work in a file that isn't doing that work: "aspirational", "this will grow to …", "Stage 4 will add …". File a ticket; don't narrate in the module.
    - ADR and module-path references ARE fine when they explain WHY a non-obvious constraint exists ("see `adr-kikan-binary-topology` — UDS is the trust boundary"). ADRs are durable; PR numbers are not.
    - When rewriting a stale comment, describe what the code **is** and the non-obvious **why** behind it. Never add a new comment that explains what just changed.

## Pre-Build Ritual

Before building any vertical: research → shaping → breadboarding → breadboard-reflection → implementation-planning → build → review.

## Deployment

```
session branches ──PR──→ main ──release──→ GitHub Releases (binary)
```

- **main** — integration branch. All PRs merge here.
- Releases are versioned binaries built from main.
- Never push directly to main.

## What NOT to Do

- No separate CSS files — Tailwind only
- No emoji icons — Lucide only
- No Svelte 4 patterns — runes only
- No `any` types in TypeScript — infer from ts-rs generated types
- No floating-point for money — Rust integer-cents or fixed-point
- No pushing to main directly
- No string-concatenated SQL — use SeaORM query builder or `sqlx::query!()` macros
- No hardcoded URLs — env vars or config only
- No bare primitive IDs — Rust newtypes for all entity identifiers
- No eslint — use `oxlint` for linting and `oxfmt` for formatting (OXC toolchain). Prettier only for `.svelte` files. Never install, configure, or run eslint.
- No shop-vertical identifiers in `crates/kikan/**` — customer, garment, quote, invoice, print_job, shop belong in `mokumo-shop` (invariant I1).
- No `tauri::` or `#[tauri::command]` under `crates/kikan/**` — any Tauri-shell-specific code lives in `kikan-tauri` or `apps/mokumo-desktop` (invariant I2). Per `adr-tauri-http-not-ipc`, the webview talks to the embedded Axum server over real HTTP, not IPC; custom `#[tauri::command]` wrappers are not used for Mokumo control or data plane logic.
- No dependency on `mokumo-shop`, `mokumo-desktop`, `mokumo-server`, or any adapter crate from inside `crates/kikan/` — DAG flows toward kikan, never away (invariant I4).
- No `DeriveEntityModel` on types in domain or wire-type modules — entities are infrastructure types; they live with their repo impl.
- No non-transactional SeaORM migrations — every migration must use `use_transaction() -> Some(true)`.
- No caret/tilde version ranges on SeaORM RC — use exact pin `"=2.0.0-rc.38"` in Cargo.toml.
- No `mokumo-decor` references — that intermediate crate is deferred per `adr-mokumo-extensions.md`; shared decoration primitives live in `mokumo-shop` until a concrete second-consumer forces extraction.
- No "auto-repair litmus test" as a design gate — that framing was retired by the extensions ADR. Mokumo IS the decoration shop app; decoration-specific concepts are welcome in `mokumo-shop` if they're not extension-specific, and in `crates/extensions/{technique}/` if they are.

## Private Knowledge

@~/.claude/mokumo-knowledge.md

## Compact Instructions

Preserve:
- Current task objective, acceptance criteria, and the milestone being worked on
- File paths of all files currently being modified
- Most recent test/build output (pass/fail, error messages)
- Active branch name and worktree context
- Which domain (customers, garments, pricing, etc.) is being worked on
- Moon task definitions if they were recently modified

Discard:
- File contents from reads older than 5 tool calls
- Search results not acted on
- Reasoning traces from abandoned approaches
- Reference repo contents after patterns have been extracted
- Old design token listings already captured in rules
