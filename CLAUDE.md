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
moon run api:dev          # Axum backend with auto-reload (depends on web:build)
moon run api:build        # Build Rust backend (depends on web:build)
moon run api:test         # Backend tests (cargo test)
moon run api:lint         # Clippy lints
moon run api:fmt          # Check Rust formatting (cargo fmt --check)
moon run api:fmt-write    # Apply Rust formatting (cargo fmt)
moon run api:gen-types    # Generate TypeScript from Rust structs (ts-rs)
moon run api:coverage     # Rust coverage report (JSON, used by CI)
moon run api:coverage-report  # Rust coverage report (HTML, local dev)
moon run api:smoke            # Hurl HTTP smoke tests (requires running server + hurl CLI)
moon run api:db-prepare   # Prepare SQLx offline cache (CI)
moon run api:deny         # Supply-chain audit (advisories, licenses, sources)
moon check --all          # Full CI: lint, test, typecheck, build across all projects
```

Underlying tools: `cargo` (Rust), `pnpm` (SvelteKit). Use directly only when diagnosing Moon task failures.

## Session Startup

- **Host sessions**: code-modifying work uses `claude --worktree` for automatic isolation. If not launched with `--worktree`, use the `EnterWorktree` tool to create one before making changes.
- **Container sessions (cmux/Docker)**: the container **is** the worktree — do NOT run `claude --worktree`, `EnterWorktree`, or `git worktree add` inside `/workspace`. Git writes the new worktree's metadata with container-only paths (e.g. `gitdir: /workspace/...`) into the bind-mounted `.git/worktrees/`, the host sees those entries as `prunable`, and any host `git worktree prune` wipes them — silently breaking every git-backed tool (`moon`, `lefthook`, `gh`) in whichever container was using that metadata. Parallelism inside a container uses sub-agents that share the same `/workspace`; for a genuinely separate workspace, stop and spin up a second host-created worktree in its own container.
- **Never push to main directly** — always branch + PR
- **Commit+push after every logical chunk** — never leave work local-only
- **Run `moon run api:deny` after touching Cargo.toml or Cargo.lock** — catches advisory, license, and supply-chain issues before CI
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
│   ├── desktop/              # Tauri v2 desktop shell (wraps Axum + SvelteKit)
│   └── web/                  # SvelteKit frontend (adapter-static)
├── services/
│   └── api/                  # Axum backend
├── crates/
│   ├── core/                 # Domain logic (pure Rust, no framework deps)
│   ├── types/                # Shared types with ts-rs derives
│   └── db/                   # SeaORM entities + repository implementations
└── tools/
    └── license-server/       # Tiny license validation function
```

## Architecture

Clean Architecture — horizontal crate layers with vertical feature organization within each layer. Crate boundaries are **compiler-enforced**: if `crates/core` doesn't list `sea-orm` or `sqlx` in its `Cargo.toml`, domain code physically cannot import database or ORM types.

**Crate layers (horizontal):**
- `crates/core/` — domain logic, trait definitions (ports), business rules. No framework deps.
- `crates/db/` — SeaORM entities and repository implementations (adapters). SeaORM for entity CRUD, raw SQLx for complex/reporting queries. Implements traits from core.
- `crates/types/` — API DTOs and wire types with `#[derive(TS, Serialize)]`. No `DeriveEntityModel` or `FromRow` here — ORM types stay in `db/`. See ADR `adr-entity-type-placement.md`.
- `services/api/` — Axum handlers, middleware, routing. Thin layer over core services.
- `apps/web/` — SvelteKit UI. Consumes TypeScript types generated by ts-rs.

**Feature organization (vertical within each crate):**
```
crates/core/src/
  customer/
    mod.rs          # Customer type, CustomerId newtype
    traits.rs       # CustomerRepository trait (port)
    service.rs      # business logic (uses trait, not impl)
  quote/
    mod.rs
    traits.rs
    service.rs

crates/db/src/
  customer/
    entity.rs       # SeaORM entity (DeriveEntityModel)
    repo.rs         # SeaORM impl of CustomerRepository
  quote/
    entity.rs
    repo.rs

services/api/src/
  customer/
    handler.rs      # Axum routes, thin
  quote/
    handler.rs
```

Build features end-to-end as vertical slices (core/customer → db/customer → api/customer → web/customer), but the crate boundaries ensure domain logic stays portable — reusable in CLI tools, WASM modules, or future crates.

**Import rule**: `core` never imports from `db` or `api`. Dependencies flow inward.

### Crate stratification (three-layer ladder)

Above the kikan/vertical split, Mokumo's application crates stratify into three layers with **additive growth** (new crates sit on top; existing crates never get extracted from). See ADR `ops/decisions/mokumo/adr-neutral-core-additive-verticals.md`.

1. **`crates/mokumo-shop/`** — **neutral shop core.** Customer CRM, shop settings, quotes, invoices, orders, kanban workflow, generic inventory (passthrough/consumable), products, cost+markup pricing engine with templates, shop financials. Passes the auto-repair-shop litmus test. No decorator semantics.
2. **`crates/mokumo-decor/`** — **future additive crate.** Artwork pipeline, garments-as-substrates, artwork-on-garment mockups, DTF gangsheet builder. Introduced when artwork + substrate work begins. Depends on `mokumo-shop` + `kikan`.
3. **Method crates** (`mokumo-screenprint`, `mokumo-embroidery`, `mokumo-dtg`, `mokumo-dtf`) — **future additive crates.** Each implements the full `PricingStrategy` + workflow stage template for its method. Depend on `mokumo-decor` + `kikan`.

**Auto-repair litmus test** (active design discipline): before adding a field, endpoint, or type to `mokumo-shop`, ask *"would an auto-repair shop need this shape?"* If no, it belongs in a vertical layer (`mokumo-decor` or a method crate), not in the core. The test governs what climbs the ladder toward the core; inside a vertical crate, decoration-specific concepts are welcome.

**Inventory classification** (GAAP-rooted): `mokumo-shop` distinguishes *passthrough* inventory (COGS-hitting, counted per-unit, appears on the invoice) from *consumable* inventory (overhead-absorbed, used up in production). A *substrate* is a specialization of passthrough (passthrough that gets modified during production) — substrate belongs in `mokumo-decor`, not core. Auto-repair parts are passthrough but not substrates.

**Extension seams are replace-points**: pricing and workflow are expressed as traits that verticals implement entirely (`PricingStrategy`, `WorkflowTemplate`). Core provides cost+markup and a generic state machine as *its own* implementation, not as a base for verticals to extend.

## Coding Standards

1. **Rust newtypes for entity IDs** — `struct CustomerId(uuid::Uuid)`, not bare `String`. Never implement `Deref`/`DerefMut` on newtypes — use `.get()` for inner access, `From`/`Into` for conversion. Keep `sea-orm` and `sqlx` derives out of `crates/core/` and `crates/types/` — `DeriveEntityModel` and `FromRow` belong only in `crates/db/` on internal types. Domain entity structs live in `core/`, API DTOs in `types/`. See ADR `adr-entity-type-placement.md`.
2. **Financial arithmetic in Rust** — money types with fixed-point or integer-cents representation. Never floating-point for prices, totals, or tax.
3. **Hybrid ORM + raw SQL** — SeaORM for entity CRUD operations, `sqlx::query!()` / `sqlx::query_as!()` for complex joins, reporting, and aggregate queries. Never string-concatenated SQL in either approach.
4. **Svelte 5 runes only** — `$state`, `$derived`, `$effect`, `$props`. Never Svelte 4 stores or `export let`.
5. **Axum patterns** — standard Axum server setup, SQLite PRAGMAs (WAL, foreign_keys, busy_timeout), `thiserror` + `IntoResponse` error handling, repository traits with `Send + Sync` bounds.
6. **ts-rs type sharing** — API DTOs in `crates/types/` derive `TS` + `Serialize` for TypeScript generation. SeaORM entities in `crates/db/` derive `DeriveEntityModel` separately — they are infrastructure types, not shared. Run `moon run api:gen-types` to regenerate TypeScript bindings.
7. **Error handling** — `thiserror` for domain errors in `crates/core/`, custom `AppError` implementing `IntoResponse` in `services/api/`.
8. **No raw SQL injection** — parameterized queries only.
9. **URL state** — filters, search, pagination in URL query params. Svelte `$state` for ephemeral UI state only.
10. **Repository traits** — `async fn` in traits (Rust 1.75+, no `async-trait` crate). Traits in `crates/core/`, impls in `crates/db/` using SeaORM. Repo impls convert between SeaORM entities (`crates/db/`) and domain types (`crates/core/`). Bounds: `Send + Sync` only.
11. **SQLite `updated_at` triggers** — every mutable table gets an `AFTER UPDATE` trigger in its migration.
12. **Activity logging is part of the mutation contract, enforced by the adapter.** Entity repository adapters in `crates/db/` insert activity log entries within the same transaction as the mutation using the shared `insert_activity_log_raw()` helper. The service layer does not orchestrate logging — atomicity is guaranteed by the adapter. Future entity verticals (garment, quote, invoice) follow this same pattern: the `_raw` helper is `pub(crate)` within `crates/db/`, callable from any entity repo adapter.
13. **No sealed traits on internal crates** — crate boundaries provide sufficient encapsulation. Sealing blocks test doubles.
14. **SeaORM entity placement** — entities with `DeriveEntityModel` belong in `crates/db/` only, never in `crates/core/` or `crates/types/`. SeaORM entities are infrastructure types; domain types in `core/` remain ORM-free. Repository impls convert between the two.
15. **SeaORM migrations** — every migration must return `Some(true)` from `use_transaction()` (atomic SQLite migrations). Pre-migration backup is non-negotiable. `updated_at` triggers still required per item 11.
16. **Pre-implementation boundary checklist** — before writing any conditional, path-matching, or range-checking code, answer four questions: (a) What are the boundary values? (b) What happens *at* each boundary? (c) What is the "almost right" input that should be rejected? (d) How does the caller see a rejected input (error code, status, message)? Each answer should have a corresponding test. See `ops/standards/testing/negative-path.md`.

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
- No SeaORM entities in `crates/core/` — entity structs with `DeriveEntityModel` are infrastructure types, not domain types
- No non-transactional SeaORM migrations — every migration must use `use_transaction() -> Some(true)`
- No caret/tilde version ranges on SeaORM RC — use exact pin `"=2.0.0-rc.38"` in Cargo.toml

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
