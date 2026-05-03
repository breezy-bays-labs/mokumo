# Mokumo Vertical Language

> Glossary of vertical-domain vocabulary used inside `crates/mokumo-shop/`, `apps/web/` (the SvelteKit frontend), and the `apps/mokumo-*` binaries. This is the language a decorator-shop owner — and the engineer building for them — uses. Platform vocabulary (Engine, Graft, Profile, Tenancy, Migration runner, …) lives in [`crates/kikan/LANGUAGE.md`](crates/kikan/LANGUAGE.md), not here.
>
> Pair with [`ARCHITECTURE.md`](ARCHITECTURE.md) for structural detail and [`CONTEXT.md`](CONTEXT.md) for navigation across the doc set.

## How to read this glossary

- Terms are grouped by surface area (setup, customer, shop, order lifecycle, …) and alphabetical within each group.
- Each entry names the canonical Rust symbol, wire-type, or frontend route (when there is one) and its location.
- **Planned terms** that map to a frontend route stub but no backend implementation yet are marked **(planned)**. They appear in the glossary so the doc evolves in lockstep with the lifecycle build-out (M1 onward) rather than racing to catch up.
- Boundary terms — vocabulary the vertical glossary shares with the platform — get their own [Boundary terms](#boundary-terms) section at the end with explicit cross-references.

> **Provenance.** This file does **not** travel with `crates/kikan/` post-beta extraction. It belongs to Mokumo. The kikan platform glossary at [`crates/kikan/LANGUAGE.md`](crates/kikan/LANGUAGE.md) is the file that travels.

---

## 1. Setup and profile

### Demo

A profile created with `SetupMode::Demo`. Pre-populated with sample data so a shop owner can explore the UI before committing to a Production profile. Reset destructively via the `Demo Reset` flow.

### MokumoApp

`mokumo_shop::graft::MokumoApp` — the Mokumo `Graft` impl. Carries `MokumoAppState`, declares `db_filename = "mokumo.db"`, and implements the kikan `Graft` contract for vertical migrations, recovery directory, and data-plane routes.

### MokumoAppState / MokumoState / MokumoShopState

`mokumo_shop::state::*` — runtime state carried through the data-plane router. Holds repository handles, the activity writer, and shop-domain services.

### Production

A profile created with `SetupMode::Production`. Real shop data; never reset destructively. Backups are mandatory before migration; recovery requires a recovery artifact in the configured `recovery_dir`.

### Setup

The first-run wizard. Triggered when `kikan::detect_boot_state` returns "pristine" and `Graft::requires_setup_wizard(profile_kind)` returns `true`. Creates the initial admin user and the first profile.

### SetupMode

`kikan_types::SetupMode` — Mokumo's `ProfileKind`. Variants: `Demo`, `Production`. The wire-type lives in `kikan-types` because both the engine and the SPA name it. See [Boundary terms](#boundary-terms).

---

## 2. Customer

### Customer

`mokumo_shop::customer::Customer` — a person or organization the shop sells to. Carries `name`, contact fields, an optional `tag` taxonomy, and `created_at` / `updated_at`. Soft-deletable via `deleted_at`.

### CustomerId

`mokumo_shop::customer::CustomerId(Uuid)` — newtype wrapping a UUID. Bare `Uuid` or `String` IDs never appear in handler signatures; the newtype is the contract.

### CustomerRepository / SqliteCustomerRepository

`mokumo_shop::customer::{CustomerRepository, SqliteCustomerRepository}` — repo trait + SeaORM impl. The SQLite impl writes activity-log rows in the same transaction as the mutation (per the `ActivityWriter` contract).

### CustomerService

`mokumo_shop::customer::CustomerService<R>` — domain service over a `CustomerRepository`. Where validation / dedup / search logic lives.

### CustomerResponse

`mokumo_shop::types::CustomerResponse` — wire shape returned by customer endpoints.

### CreateCustomer / UpdateCustomer

Wire shapes for create / update requests.

---

## 3. Shop

### Shop

`mokumo_shop::shop::Shop` — the tenant's own brand identity inside a profile: name, address, tax ID, and contact information. One `Shop` row per profile.

### Shop Logo

The shop's logo image. Validated by `LogoValidator` (format, size, dimensions). Stored on disk under the profile's data directory; pointers in `meta.db` are backed up alongside the SQLite file.

### LogoFormat / LogoError

`mokumo_shop::shop::{LogoFormat, LogoError}` — supported formats (`PNG`, `JPEG`, `SVG`, …) and the validation-error union.

### LogoValidator / ValidatedLogo

`mokumo_shop::shop::{LogoValidator, ValidatedLogo}` — validator + validated newtype. Production code never accepts a raw `Bytes` blob; it accepts a `ValidatedLogo`.

### ShopLogoService / ShopLogoRepository

`mokumo_shop::shop::{ShopLogoService, ShopLogoRepository}` — service + repo trait for the logo artifact pipeline (write / fetch / replace / delete).

---

## 4. Sequence

### FormattedSequence

`mokumo_shop::sequence::FormattedSequence` — display-shaped sequence number after applying the configured prefix, padding, and increment rules.

### SequenceGenerator / SqliteSequenceGenerator

`mokumo_shop::sequence::{SequenceGenerator, SqliteSequenceGenerator}` — atomically-incrementing generator for shop-wide series (Order #, Invoice #, …). The SQLite impl uses an `INSERT … ON CONFLICT DO UPDATE … RETURNING` upsert against `number_sequences`, so allocation is single-statement and contention-safe.

---

## 5. Order lifecycle

The garment lifecycle: **Quote → Artwork Approval → Production → Shipping → Invoice**. The frontend route shells exist under `apps/web/src/routes/(app)/{orders,quotes,artwork,production,shipping,invoices}`; the backend domain types are introduced milestone-by-milestone. Where a term is **(planned)**, the frontend stub exists but the Rust domain type does not yet.

### Order **(planned)**

The top-level container for a customer's work. Holds the lifecycle state, the line items, the customer, the financials, and a sequence number. Lands in M1.

### Quote **(planned)**

A pre-production pricing artifact. Once accepted, becomes the source-of-truth for the Order's pricing. Lands in M1 Wave 1 (Quote Foundation + Simple Merch — issue #173).

### Artwork Approval **(planned)**

The approval gate between Quote acceptance and Production. The customer reviews mockups; once approved, the order proceeds to Production. Frontend route stub at `apps/web/src/routes/(app)/artwork`; backend lands in M1 / M2.

### Production **(planned)**

The manufacturing stage. A garment moves through Production while it is being decorated. Frontend route stub at `apps/web/src/routes/(app)/production`; backend lands in M1 / M2.

### Job **(planned)**

A unit of production work — typically one decoration pass on one garment line. An Order may have multiple Jobs (front-print + back-print + tag-print). Lands with Production in M1 / M2.

### Kanban **(planned)**

The board view of work-in-progress Jobs and Orders. Frontend uses card-per-Order columns keyed on lifecycle state. Lands with Production.

### Shipping **(planned)**

The fulfillment stage. Tracks carriers, tracking numbers, ship-by-date, and partial shipments. Frontend route stub at `apps/web/src/routes/(app)/shipping`.

### Invoice **(planned)**

The billing artifact. Generated from the accepted Quote at Order completion or at scheduled milestones. Frontend route stub at `apps/web/src/routes/(app)/invoices`.

---

## 6. Decoration model

How decoration techniques (screen print, embroidery, DTF, DTG, sublimation, …) compose into the shop. The full mechanism lands in Wave 8 per [`ARCHITECTURE.md` §9](ARCHITECTURE.md#9-extension-model-placeholder-until-wave-8). Vocabulary is captured here so the doc lines up when the code arrives.

### Decoration Method **(planned)**

The technique applied to a Garment — `ScreenPrint`, `Embroidery`, `DTF`, `DTG`, `Sublimation`, `HeatTransfer`, `DirectToGarment`. Each method is its own extension crate at `crates/extensions/{method}/` and is registered against `mokumo-shop`'s `ExtensionRegistry`.

### Extension **(planned)**

A crate at `crates/extensions/{technique}/` that contributes domain types, side tables (never extends shop base tables), and per-method pricing rules to `mokumo-shop`. Extensions register at `BootConfig` construction time and are activated per-profile via `meta.db.profile_active_extensions`.

### Garment **(planned)**

The physical apparel item being decorated — t-shirt, hoodie, hat, polo. The substrate of the work. Frontend route stub at `apps/web/src/routes/(app)/garments`. The backend Garment domain type lands in M1.

### Line / LineKind **(planned)**

A single line item on a Quote / Order / Invoice. The kind determines which extension owns the line's pricing — `LineKind::ScreenPrint` dispatches to the screen-printing extension. Per the Backstage-style typed dispatch decision documented in `adr-mokumo-extensions`.

### Methodology **(planned)**

How decoration is applied (screen print, embroidery, DTF, …). Independent axis from substrate — an extension declares which (substrate × methodology) combinations it supports.

### Substrate **(planned)**

What is being decorated — apparel (garments), hard goods (mugs, tumblers), banners. Independent axis from methodology.

---

## 7. Activity actions

The Mokumo-vertical-emitted variants of `kikan_types::ActivityAction`. Adapters insert these inside the same transaction as the mutation they record.

| Action | Triggered by |
|---|---|
| `CustomerCreated` | `SqliteCustomerRepository::create` |
| `CustomerUpdated` | `SqliteCustomerRepository::update` |
| `CustomerDeleted` | `SqliteCustomerRepository::soft_delete` |
| `ShopLogoSet` | `SqliteShopLogoRepository::write` |
| `ShopLogoCleared` | `SqliteShopLogoRepository::delete` |

Platform-shaped actions (login, profile-switch, backup, restore, recovery) live in [`crates/kikan/LANGUAGE.md` §5](crates/kikan/LANGUAGE.md#5-activity-and-audit).

---

## 8. Demo Reset

### Demo Reset

A destructive reset of a `Demo` profile back to the seeded sample-data state. Uses kikan's `SidecarRecovery` mechanism: the engine swaps the live `mokumo.db` for a fresh seeded sidecar atomically. Never available on `Production` profiles. The control-plane endpoint is gated by `Graft::requires_setup_wizard` and the profile's kind.

### RecoveryCleanupError

`mokumo_shop::demo_reset::RecoveryCleanupError` — error union for sweeping stale recovery artifacts during a reset.

---

## 9. CLI entry points

### kikan-cli

The admin CLI binary, dispatched as a subcommand of `mokumo-server`. Talks to the running server over a Unix domain socket at mode 0600. Reachable only from the same host (physical access is the trust boundary). Subcommands cover profile lifecycle, backup / restore, diagnostics, and user administration.

### mokumo-server

The headless Linux/musl-compatible binary. Composes `kikan` + `kikan-socket` + `kikan-cli` + `kikan-spa-sveltekit` + `kikan-types` + `mokumo-shop`. Zero transitive Tauri dependency (invariant I3).

### mokumo-desktop

The Tauri desktop binary. Composes `kikan` + `kikan-tauri` + `kikan-spa-sveltekit` + `mokumo-shop` + `kikan-types`. Native window, tray, single-instance, auto-updater.

---

## Boundary terms

Vocabulary that the vertical glossary shares with [`crates/kikan/LANGUAGE.md`](crates/kikan/LANGUAGE.md) — the seam where vertical language meets platform language.

| Term | Vertical side | Platform side |
|---|---|---|
| **Profile / ProfileKind** | Mokumo's `ProfileKind` is `kikan_types::SetupMode` (`Demo`, `Production`). The wire-type lives in `kikan-types` because the SPA names it. See §1 above. | Kikan owns the `Profile` row, repository, lifecycle, and tenancy resolution. See [`crates/kikan/LANGUAGE.md` §2](crates/kikan/LANGUAGE.md#2-tenancy-and-profiles). |
| **Active DB** | `MokumoApp::db_filename` returns `"mokumo.db"`. The vertical chooses the filename. | Kikan owns init, pragmas, pool, migration runner, and backup. See [`crates/kikan/LANGUAGE.md` §2](crates/kikan/LANGUAGE.md#2-tenancy-and-profiles). |
| **Migration** | Mokumo's per-profile migrations live under `crates/mokumo-shop/src/migrations/` and are contributed via `MokumoApp::migrations`. They name vertical tables (`customers`, `shop`, `number_sequences`, `activity_log`, …). | Kikan owns the `Migration` trait, the per-profile DAG runner, transactional application, and `MigrationTarget` routing. See [`crates/kikan/LANGUAGE.md` §6](crates/kikan/LANGUAGE.md#6-migrations-backups-restore). |
| **User** | Mokumo's auth handlers (login / forgot-password / recover / reset / regenerate-recovery-codes) sit in `crates/mokumo-shop/src/auth/`. They route through the kikan `Backend` and never touch the user model. | Kikan owns `User`, `UserId`, `Backend`, `UserService`, password hashing, and login rate-limiting. See [`crates/kikan/LANGUAGE.md` §4](crates/kikan/LANGUAGE.md#4-auth-sessions-and-recovery). |
| **Activity Log** | Mokumo emits log rows from adapter-layer mutations (see §7 above). Action variants for vertical mutations live in `kikan_types::ActivityAction` so the SPA can name them too. | Kikan owns the table shape, the `ActivityWriter` trait, and the SQLite impls. See [`crates/kikan/LANGUAGE.md` §5](crates/kikan/LANGUAGE.md#5-activity-and-audit). |
| **Recovery Artifact** | `MokumoApp::recovery_dir(profile_id)` chooses the directory; `mokumo_shop::auth::recovery_artifact` defines the file format. | Kikan owns the watcher, the validation mechanism, and `RecoveryArtifactLocation`. See [`crates/kikan/LANGUAGE.md` §4](crates/kikan/LANGUAGE.md#4-auth-sessions-and-recovery). |
| **Setup Token** | `MokumoApp::requires_setup_wizard(profile_kind)` returns `true` for `Demo` and `Production`. | Kikan owns the `SetupTokenSource` flow and the bootstrap. See [`crates/kikan/LANGUAGE.md` §4](crates/kikan/LANGUAGE.md#4-auth-sessions-and-recovery). |

---

## When this glossary is wrong

If a term in this file disagrees with the code: the code wins, this file is stale. Open a PR that updates the glossary in the same diff as the code change. The Synchronized-Docs rule in [`AGENTS.md`](AGENTS.md#synchronized-docs) calls this out as a paired-files invariant — a `pub` symbol added under `crates/mokumo-*` should arrive with its glossary entry, not "in a follow-up."
