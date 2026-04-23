# Mokumo Architecture

> Source of truth for how Mokumo is built. New contributors and agents should read this end-to-end before touching code. This document is platform-first: Kikan (the embedded application engine) is the primary subject, and Mokumo is described as the first vertical application built on it.

For the day-to-day "where do I find things, what commands do I run" perspective, see [CLAUDE.md](CLAUDE.md). For the agent-facing per-crate briefs, see each crate's `AGENTS.md`. For decision rationale, see ADRs in the private `ops/decisions/mokumo/` repo (referenced inline below).

---

## 1. Scope: two products in one repo

This repository ships two products from a single Cargo + pnpm workspace:

- **Kikan** (基幹, "backbone / foundation trunk / core system") — a self-hosted Rust application platform. Reusable engine that handles tenancy, migrations, backup/restore, auth, sessions, control-plane administration, mDNS LAN discovery, deployment-mode awareness, and platform-wide observability primitives. Headless-first; never assumes a particular UI shell.
- **Mokumo** — a decorator garment management application. Quote → Artwork Approval → Production → Shipping → Invoice. The first vertical built on Kikan and the only vertical in this repo today.

Kikan stays inside the Mokumo repo through Mokumo's beta launch (M16) per [adr-workspace-split-kikan §monorepo-through-beta amendment](https://github.com/breezy-bays-labs/mokumo/blob/main/ARCHITECTURE.md). Repo extraction is deferred until Kikan has a second consumer; goal-driven, not date-driven.

The architectural intent is that any future self-hosted Rust application (a second vertical, a different domain entirely) should be able to plug into Kikan via the `Graft` trait without modifying the platform. Section [§5 The Graft / SubGraft pattern](#5-the-graft--subgraft-pattern) describes the contract.

---

## 2. Workspace crate map

The workspace contains 11 library crates and 2 binary apps. Their dependency direction is enforced by CI invariant I4 (see [§8 Quality invariants](#8-quality-invariants)).

![Workspace crate DAG](docs/diagrams/crate-dag.svg)

### Crate roles

| Crate | Role | Notes |
|---|---|---|
| `crates/kikan` | **Engine** | Tenancy, migration runner, auth (repo + backend + sessions), activity log, backup/restore primitives, control-plane handlers, platform routes (auth, health, users), event bus types, mDNS, `Engine<G: Graft>`. **Knows nothing about Mokumo** (invariant I1, regex-enforced). |
| `crates/kikan-types` | Wire DTOs | Serde + ts-rs types shared between the Rust API and the SvelteKit frontend. |
| `crates/kikan-tauri` | Tauri-shell helpers | Ephemeral-port binding, bundle-ID templating, window setup. **Has no `tauri::` symbol in its public API of `kikan`** (invariant I2). |
| `crates/kikan-cli` | Admin CLI library | Clap subcommands + UDS HTTP client. Subcommand-dispatched by `mokumo-server`. |
| `crates/kikan-socket` | UDS listener primitives | Mode-0600 daemon listener + permission enforcement. Unix-only. Zero workspace-local deps. |
| `crates/kikan-events` | SubGraft | Tokio broadcast wrapper + `EventBus` trait + `EventBusSubGraft`. |
| `crates/kikan-mail` | SubGraft | `Mailer` trait + `LettreMailer` (prod) + `CapturingMailer` (tests) + `MailerSubGraft`. Zero workspace-local deps. |
| `crates/kikan-scheduler` | SubGraft | `Scheduler` trait + `ApalisScheduler` + `ImmediateScheduler` + `SchedulerSubGraft`. |
| `crates/mokumo-shop` | **Application (vertical)** | Shop domain + `MokumoApp: Graft` impl, lifecycle hooks, data-plane router composition, BDD/HTTP integration suite. Customer, shop, sequences, quotes, invoices, kanban, products, generic inventory, cost+markup pricing, vertical migrations. |
| `crates/kikan-spa-sveltekit` | SPA adapter | Two `SpaSource` impls for `kikan::data_plane::spa::SpaSource`: `SvelteKitSpa<A: RustEmbed>` for embedded single-binary builds and `SvelteKitSpaDir { dir }` for on-disk serving. Owns the SvelteKit cache policy. Consumers pick it at their edge so `kikan` stays `rust-embed`-free (invariant I5). |
| `crates/mokumo-core` | Neutral infrastructure | `thiserror`, pagination types, activity base. **No shop vocabulary**. Currently consumed by `kikan` and `kikan-types` as a transitional dependency; will fold into Kikan when those primitives migrate. |
| `apps/mokumo-desktop` | Tauri binary | Composes `kikan` + `kikan-tauri` + `kikan-spa-sveltekit` + `mokumo-shop` + `mokumo-core` + `kikan-types`. Owns its own `#[derive(rust_embed::Embed)] struct SpaAssets` and injects `SvelteKitSpa<SpaAssets>` via `MokumoApp::with_spa_source(...)`. Native window, tray, single-instance, auto-updater, native dialogs. |
| `apps/mokumo-server` | Headless binary | Composes `kikan` + `kikan-socket` + `kikan-cli` + `kikan-spa-sveltekit` + `kikan-types` + `mokumo-shop`. `--spa-dir <PATH>` injects `SvelteKitSpaDir`; absent flag runs API-only. **Zero transitive Tauri dependency** (invariant I3, CI-enforced via `cargo tree`). |

### Dependency rules

- All adapter and SubGraft crates point at `kikan`. None depends on `mokumo-shop` or any binary.
- `mokumo-shop` depends on `kikan` (and `kikan-types`, `mokumo-core`). It does not depend on any adapter or SubGraft crate — including `kikan-spa-sveltekit`; binaries inject a concrete `SpaSource` via `MokumoApp::with_spa_source(...)` so the shop vertical never imports the SPA adapter.
- `kikan` has no workspace-local edge into any shop / adapter / binary crate. Its only workspace edge is the transitional one to `mokumo-core` shown in red on the diagram.
- Binaries compose multiple crates; binaries are the only place where Tauri ↔ Mokumo ↔ kikan ↔ adapters meet.

The full canonical edge list (machine-readable) lives in each crate's `Cargo.toml` `[dependencies]` block; `scripts/check-i4-dag.sh` enforces no edges from `kikan/src/` toward forbidden crates.

---

## 3. Deployment topology

Kikan ships into three deployment-trust models. Mokumo packages those models into two binary shapes: a Tauri desktop bundle for non-technical shop owners, and a headless server binary for self-hosters running on a NAS, VM, container, or Tailscale node.

![Deployment topology](docs/diagrams/deployment-topology.svg)

### Two binaries, one router

`apps/mokumo-desktop` is a Tauri shell that spawns the same Axum server the headless `apps/mokumo-server` runs. The Tauri webview points at `http://localhost:PORT`. There is no separate IPC handler set; admin actions go over HTTP-on-loopback, and the webview is the only client allowed onto the loopback origin.

`apps/mokumo-server` is the headless equivalent. Linux/musl-compatible, container-friendly, suitable for self-hosters who want to run Mokumo on a home server and connect from the LAN. The CLI (`kikan-cli`) talks to the running server over a Unix domain socket at mode 0600 — physical filesystem access is the trust boundary.

This is the "HTTP-everywhere" decision documented in `ops/decisions/mokumo/adr-tauri-http-not-ipc.md`. It costs a small amount of duplicated transport overhead vs pure IPC, and buys: identical handler code paths between desktop and headless, no `#[tauri::command]` proliferation, trivial LAN access from any browser, and a clean future path to remote access via Tailscale or Cloudflare tunnel without rewriting the admin surface.

### `DeploymentMode` knob matrix

`kikan::DataPlaneConfig::deployment_mode` selects the trust posture at boot. The middleware stack reads from it; nothing else has to know.

| Mode | Trust model | Session cookies | CSRF | Rate limit | mDNS | Host allow-list |
|---|---|---|---|---|---|---|
| **Lan** | physical network access | `Secure=false`, `SameSite=Lax` | off | per-user only | on | loopback + `{shop}.local` |
| **Internet** | public-exposed direct | `Secure=true`, `SameSite=Lax` | on | per-user + per-IP | off | configured domain |
| **ReverseProxy** | behind Caddy / nginx / Traefik | `Secure=true` (proxy-controlled) | on | per-user + per-IP | off | via `X-Forwarded-Host` |

A 4th mode `TailscaleMesh` (Tailnet-peer-attested) is post-M00 work — see the watchlist in the ops repo.

The full ADR is `ops/decisions/mokumo/adr-kikan-deployment-modes.md`. Implementation lands in extraction Session 4 (#500).

---

## 4. Control plane vs data plane

The single Axum router serves two logical surfaces with different trust boundaries.

![Control plane vs data plane](docs/diagrams/control-data-plane.svg)

- **Control plane** — admin operations: profile lifecycle, migrate / dry-run, backup / restore, diagnostics, audit, PIN recovery. Implemented as pure-Rust functions in `kikan::control_plane::*`. Reachable only over loopback (Tauri webview) or the Unix socket (CLI). Remote and LAN clients never reach the control plane; the host-allow-list middleware blocks the path.
- **Data plane** — shop business surface: customers, quotes, invoices, kanban, products, inventory, sessions, auth. Reachable from all configured client surfaces per `DeploymentMode`. Vertical routes contributed by `MokumoApp::data_plane_routes`; platform routes (auth, health, users) contributed by Kikan.

### Why "pure-Rust handlers + thin transports"

Three independent transports (Tauri webview, headless HTTP, UDS) need to drive the same admin operations. If admin operations were `#[tauri::command]` functions or Tauri IPC handlers, the headless binary would need a parallel implementation. By keeping `kikan::control_plane` as plain `async fn` returning `Result<T, ControlPlaneError>`, every transport adapter is a thin wrapper.

This is invariant I2 (adapter boundary): no `tauri::` types appear in `crates/kikan/**`. CI enforces it via `scripts/check-i2-adapter-boundary.sh`. The full ADR is `ops/decisions/mokumo/adr-control-plane-data-plane-split.md`.

---

## 5. The Graft / SubGraft pattern

Kikan provides the **mechanism** for platform features (backups, migrations, diagnostics, control plane, auth, activity logging). The application — via the `Graft` trait — provides the **specifics** Kikan can't know.

### `Graft` — the application's contract with the engine

```rust
pub trait Graft: Send + Sync + 'static {
    /// App-chosen state type. Kikan carries it opaquely.
    type AppState: Clone + Send + Sync + 'static;

    /// App-chosen profile-kind enum. Kikan stores and routes; doesn't interpret.
    /// Mokumo sets `type ProfileKind = SetupMode;`. A different app picks its own.
    type ProfileKind: Clone + Send + Sync + 'static;

    /// Filename of this app's primary per-profile database.
    /// Used by kikan for backups, migrations, diagnostics.
    fn db_filename(&self) -> &'static str;

    /// Directory kikan watches for recovery-PIN file drops for this profile.
    /// Kikan owns the watching + validation mechanism; app owns the location.
    fn recovery_dir(&self, profile_id: &ProfileId) -> PathBuf;

    /// App's per-profile migrations (in addition to kikan's platform migrations).
    fn migrations(&self) -> Vec<Box<dyn Migration>>;

    /// App's data-plane routes (customer, quote, ...).
    /// Kikan composes its platform routes + these + SubGraft routes.
    fn data_plane_routes(&self, state: Self::AppState) -> Router<Self::AppState>;
}
```

This is **dependency inversion through a trait with associated types**. Kikan depends on the abstraction; Mokumo (or any other vertical) supplies the concrete values:

```rust
// crates/mokumo-shop/src/graft.rs
impl Graft for MokumoApp {
    type AppState = MokumoAppState;
    type ProfileKind = SetupMode;            // Mokumo's own enum
    fn db_filename(&self) -> &'static str { "mokumo.db" }
    fn recovery_dir(&self, profile_id: &ProfileId) -> PathBuf { ... }
    fn migrations(&self) -> Vec<Box<dyn Migration>> { ... }
    fn data_plane_routes(&self, state: Self::AppState) -> Router<Self::AppState> { ... }
}
```

And the binary composes:

```rust
// apps/mokumo-desktop/src/lib.rs
let engine = Engine::boot(
    BootConfig::new(MokumoApp::new())
        .with_deployment_mode(DeploymentMode::Lan)
        .with_subgraft(EventBusSubGraft::new(BroadcastEventBus::default()))
)?;
```

### `SubGraft` — opt-in platform satellites

SubGrafts are independent crates that contribute optional platform features. Each implements the `SubGraft` trait; apps register whichever ones they need at `BootConfig` construction time.

```rust
BootConfig::new(MokumoApp::new())
    .with_subgraft(EventBusSubGraft::new(BroadcastEventBus::default()))
    .with_subgraft(MailerSubGraft::new(LettreMailer::from_env()?))
    // scheduler not registered — app doesn't need background jobs yet
```

**Why this instead of Cargo features**: Cargo features are compile-time. The same binary can run in different deployment configurations at runtime (e.g. CLI `--mailer=smtp` vs `--mailer=disabled`). Builder-time `with_subgraft` is more flexible, stays type-safe, and doesn't make apps reason about feature flags.

Current SubGraft crates (scaffolds present, consumers wire as features arrive):

- `kikan-events` — `BroadcastEventBus`, `Event` enum, `EventBusSubGraft`. Consumer-when-needed: WebSocket migration (#622), HealthResolver (#522), audit trail (#377).
- `kikan-mail` — `Mailer` trait, `LettreMailer` (prod), `CapturingMailer` (tests), `MailerSubGraft`. Consumer-when-needed: opt-in critical-event email (#524).
- `kikan-scheduler` — `Scheduler` trait, `ApalisScheduler`, `ImmediateScheduler`, `SchedulerSubGraft`. Consumer-when-needed: scheduled backups (#428).

Full ADR: `ops/decisions/mokumo/adr-kikan-engine-vocabulary.md`.

---

## 6. Database topology

Kikan owns platform-wide state in `meta.db`. Each profile gets its own application database whose filename comes from `Graft::db_filename()` (Mokumo returns `"mokumo.db"`). Backups snapshot both.

![Database topology](docs/diagrams/db-topology.svg)

### Why this layout

- **Platform state is uniform.** Every Kikan-grafted app has the same `meta.db` shape: users, sessions, profiles, schema_version, profile_active_extensions, activity_log. New verticals get this for free.
- **Per-profile isolation.** A shop with a demo profile and a production profile gets two physically separate `mokumo.db` files. Reset / restore / corruption on one never touches the other.
- **Ephemeral session state** lives in `session.db` (tower-sessions). Login / logout doesn't touch the platform or vertical DBs.
- **Backup scope = `meta.db` + active per-profile DB.** Both written via `VACUUM INTO` for consistent snapshots without WAL/SHM coordination.
- **Recovery PIN drops** live in `Graft::recovery_dir(profile_id)`. Kikan owns the watching and validation mechanism; the app picks the directory.

### Multi-tenant SQLite, not multi-database SQL

The meta DB / per-profile DB split is a Kikan design decision per `ops/decisions/mokumo/adr-control-plane-data-plane-split.md`. It lets the platform's connection-pool management, migration runner, and backup layer treat each DB as an independent unit while still sharing platform-level identity (a user in `meta.db` can be granted access to multiple per-profile DBs).

---

## 7. Upgrade safety model

Mokumo is intended to be installed by small businesses that treat their data as the source of truth for their business. An update that corrupts data is catastrophic. The upgrade safety model is the load-bearing answer.

### Forward-fix-only with backup-before-migrate

- **Migration direction**: forward-only. Down-migrations on embedded SQLite are fragile; Kikan does not support them. Schema-incompatible bugs are fixed by a *new* migration, not by reverting.
- **Backup before every migration batch.** On `Engine::boot`, if the binary's expected schema version differs from the on-disk version, Kikan writes a `VACUUM INTO`-snapshot of `meta.db` and the active per-profile DB to `backups/{db}.bak-vX.Y.Z-{timestamp}.db` *before* applying any migration.
- **Transactional migration application.** Each migration runs inside `use_transaction() -> Some(true)` (atomic SQLite migrations). The runner acquires `max_connections` simultaneously to ensure FK=ON applies uniformly across the pool.
- **Refuse-to-downgrade.** If the on-disk schema version is *newer* than the binary supports, `Engine::boot` errors out with a clear message and the user is told which version to install. No data is touched.
- **N-2 → N compatibility contract.** Each release supports migrations from at least N-2 versions back. CI runs upgrade tests against fixture databases from prior versions (#371, Wave 1).

### `kikan::Migration` trait

Migrations are Rust types implementing the `Migration` trait, contributed via `Graft::migrations()` (vertical-owned), `SelfGraft::migrations()` (kikan platform tables), and SubGraft contribution. The runner composes a per-profile DAG and applies it in dependency order. Full ADR: `ops/decisions/mokumo/adr-kikan-upgrade-migration-strategy.md`.

### Cross-cutting Wave 1 deliverables

- `#371` — N-2 → N upgrade fixture tests
- `#540` — Meta / Backup `MigrationTarget` routing
- `#542` — `migrate --dry-run` mode
- `#428` — user-configurable scheduled backups (consumes `kikan-scheduler`)
- `#566` — migration content-hash verification (detect edited historical migrations)

---

## 8. Quality invariants

CI enforces five workspace-wide invariants. Each is a small grep / `cargo tree` script under `scripts/`.

| ID | What it enforces | Script |
|---|---|---|
| **I1** | `crates/kikan/src/` contains no shop-vertical identifiers (`customer`, `garment`, `print_job`, `quote`, `invoice`, `decorator`, `embroidery`, `dtf`, `screen.print`, `apparel`). | `scripts/check-i1-domain-purity.sh` |
| **I2** | No `tauri::` or `#[tauri::command]` symbols under `crates/kikan/**` or any non-`kikan-tauri` crate. | `scripts/check-i2-adapter-boundary.sh` |
| **I2b** | No PascalCase Tauri type identifiers leak through generic re-exports into kikan/satellites/shop. | `scripts/check-i2b-tauri-type-ids.sh` |
| **I3** | `cargo tree -p mokumo-server` (default and `--no-default-features`) shows no Tauri crate. The headless binary is Tauri-free. CI also compiles it against `x86_64-unknown-linux-musl`. | `scripts/check-i3-headless.sh` + `kikan-musl-build` job in `.github/workflows/quality.yml` |
| **I4** | One-way DAG: `kikan` has no incoming workspace-local edges from `mokumo-shop`, `mokumo-desktop`, `mokumo-server`, `kikan-tauri`, `kikan-cli`, `kikan-socket`, `kikan-events`, `kikan-mail`, or `kikan-scheduler`. | `scripts/check-i4-dag.sh` |
| **I5** | No Cargo feature anywhere reachable into `kikan` pulls Tauri. `\btauri\b` absent from `crates/kikan/Cargo.toml`. | `scripts/check-i5-features.sh` |

### Test posture

- **BDD-heavy.** Cucumber-rs in Rust + Playwright BDD steps in TypeScript. `.feature` files are the living spec; step definitions wire them to executable code.
- **CRAP-gated.** `crap4rs` runs in CI per crate; PRs that raise any function above the project threshold either refactor in-session or file a follow-up at closeout.
- **Mutation-tested.** Stryker on the SvelteKit side; `cargo-mutants` adoption is on the Wave 6 (Testing & Quality, cross-cutting) roadmap.
- **Hurl smoke tests** for every API endpoint; `tests/api/<domain>/<endpoint>.hurl` files. Error shape is `{"code":"...", "message":"...", "details":null}` — assertions check `$.code`, not `$.error`.
- **Negative-path testing standard** at `ops/standards/testing/negative-path.md` — for any conditional / path-matching / range-checking code, write the boundary cases and the "almost right" rejection case before the happy path.

---

## 9. Extension model (placeholder until Wave 8)

Decoration techniques (screen printing, embroidery, DTF, DTG, sublimation, ...) compose into `mokumo-shop` through a typed `ExtensionRegistry` with per-`LineKind` dispatch (Backstage-style). Each technique lives in its own crate at `crates/extensions/{technique}/` and is introduced one at a time, beginning at Mokumo's M4 milestone (post-M00).

Key contracts (target shape; implementation lands in Wave 8 — see `ops/vision/mokumo/milestones/m0-foundation.md`):

- Extensions register against `mokumo-shop` via `BootConfig::with_extension(MyTechniqueExtension::new())`.
- Per-profile activation lives in `meta.db.profile_active_extensions`; control-plane handlers manage activation state.
- Extensions own their own side tables. They never extend `mokumo-shop` base tables — schema isolation simplifies upgrade paths and lets a profile activate / deactivate techniques without data migration.
- Substrate (what's being decorated — apparel, hats, mugs, tumblers) and methodology (how it's decorated — screen print, embroidery, DTF) are independent axes; an extension declares which substrate × methodology combinations it supports.

Full ADR: `ops/decisions/mokumo/adr-mokumo-extensions.md`.

This document will get a real "extension model" section once Wave 8 lands.

---

## 10. Mokumo vertical (the first consumer)

`mokumo-shop` implements `Graft` for the decorator garment management domain:

- **Customer / shop / sequences** — multi-shop tenancy at the application layer; sequence numbering for orders, invoices, etc.
- **Quotes / artwork approval / production / kanban / shipping / invoices** — the garment lifecycle workflow.
- **Generic inventory + cost+markup pricing** — neutral foundations for the per-technique extensions to layer on top.
- **Frontend** — SvelteKit (Svelte 5 runes only) + Tailwind v4 + shadcn-svelte, compiled to a static SPA via `adapter-static`. Wire types come from `kikan-types` via ts-rs; never `any`.

Coding standards specific to the vertical (newtypes for entity IDs, integer-cents money, async-fn-in-traits, activity-logging-as-mutation-contract, etc.) live in [CLAUDE.md §Coding Standards](CLAUDE.md#coding-standards).

---

## 11. Decision index

These ADRs are the load-bearing decisions behind this architecture. They live in `ops/decisions/mokumo/` (private repo) and may not be world-readable; the Y-statement summary below carries the load when the link is dead.

| ADR | Y-statement summary |
|---|---|
| `adr-workspace-split-kikan` | *In the context of building Mokumo as a self-hosted Rust app, **facing** the risk of conflating engine and vertical concerns, **we** carved Kikan out as a reusable platform crate with five CI-enforced invariants (I1–I5), **so that** the engine stays headless / Tauri-free / domain-pure and a second consumer can graft on without touching it.* |
| `adr-control-plane-data-plane-split` | *In the context of admin operations vs business endpoints, **facing** the risk that admin actions get exposed over the network or duplicated across transports, **we** keep control-plane handlers as pure Rust functions reachable only over loopback (webview) or UDS (CLI), **so that** physical access remains the trust boundary for admin actions.* |
| `adr-tauri-http-not-ipc` | *In the context of how the Tauri webview talks to the backend, **facing** the risk of duplicating handler logic between IPC and HTTP transports, **we** chose HTTP-everywhere and pinned the decision with eight binding mitigation commitments, **so that** the same Axum router serves desktop, headless, LAN, and remote.* |
| `adr-kikan-engine-vocabulary` | *In the context of how Mokumo plugs into Kikan, **facing** the risk that vertical-specific naming creeps into the platform, **we** defined the `Engine`/`Application`/`Graft`/`SubGraft`/`SelfGraft` vocabulary with associated types for app-chosen state and profile kinds, **so that** every kikan ↔ vertical interaction has a named contract.* |
| `adr-kikan-binary-topology` | *In the context of distributing Mokumo, **facing** the need to support both non-technical desktop installs and headless self-hosters, **we** ship two binary crates (`mokumo-desktop` Tauri shell + `mokumo-server` headless) that compose the same Kikan engine + Mokumo vertical, **so that** the same code path serves both audiences.* |
| `adr-kikan-deployment-modes` | *In the context of where Mokumo gets deployed, **facing** different trust postures for LAN / public / proxy deployments, **we** parameterize cookie flags, CSRF, rate-limiting, mDNS, and host allow-list off a single `DeploymentMode` enum, **so that** the same binary runs safely in every shape with one runtime knob.* |
| `adr-kikan-upgrade-migration-strategy` | *In the context of small-business shops trusting Mokumo with their data, **facing** the catastrophic cost of an upgrade that corrupts data, **we** mandate forward-fix-only migrations + backup-before-migrate + transactional application + refuse-to-downgrade + N-2 → N fixture tests, **so that** an update that goes wrong is recoverable from a snapshot Kikan took moments earlier.* |
| `adr-kikan-release-channels` | *In the context of shipping updates safely, **facing** the need for canary releases without exposing every user to in-flight bugs, **we** define stable + beta channels via Cargo features and bundle-id templating with auto-updater integration and Windows code signing, **so that** users opt into the risk profile they want.* |
| `adr-mokumo-extensions` | *In the context of decoration techniques (screen print, embroidery, DTF, DTG), **facing** the risk that mokumo-shop becomes a kitchen sink, **we** compose techniques as independent extension crates registered against an `ExtensionRegistry` with per-profile activation, **so that** verticals add capability without modifying mokumo-shop schema.* |
| `adr-entity-type-placement` | *In the context of where SeaORM `DeriveEntityModel` types live, **facing** the risk of leaking infrastructure types into wire DTOs, **we** keep entities with their repo impl in whichever crate owns the data, never in `kikan-types` and never in domain-pure modules, **so that** ts-rs-generated TypeScript stays free of ORM concerns.* |
| `adr-platform-framework-posture` | *In the context of evaluating Rust web frameworks, **facing** the temptation to swap Axum for Loco or another full-stack, **we** stay on Axum and run a Late-M00 Loco hardening pass to KEEP/ADOPT/BACKLOG patterns from richer frameworks, **so that** we keep our composition flexibility and pull in proven ergonomics deliberately.* |

For the full ADR text, browse `ops/decisions/mokumo/` (private). The corpus contained 70 ADRs as of 2026-04-19; the table above lists only the ones load-bearing for understanding this document.

---

## How to update this document

1. **Architectural change** (new crate, new boundary, new invariant, ADR amendment): update the relevant section here in the same PR as the code. The diagrams under `docs/diagrams/` are D2 source — re-render with `d2 docs/diagrams/<name>.d2 docs/diagrams/<name>.svg` and commit both. CI fails on diagram drift.
2. **New ADR** ratified: add a row to §11 with a Y-statement summary.
3. **Deprecation / supersession**: don't delete the section — strike through and link to the replacement, with a date. This document is also a history of what we used to do.

When in doubt, ask: would a contributor with no access to private repos still understand the architecture from this document alone? If the answer is no, fix the document, not the contributor's access.
