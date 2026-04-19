# SPA Embedding Boundary Decision — mokumo-api × mokumo-server

## Date

2026-04-19

## Status

Pending second opinion (Gemini review)

---

## Context: What We're Building

Mokumo is a self-hosted production management app for decorated apparel shops. The architecture follows a multi-stage "kikan extraction" that separates:

- **Engine** (`crates/kikan/`) — tenancy, migrations, auth, platform handlers. Zero vertical-domain knowledge (invariant I1).
- **Application** (`crates/mokumo-shop/`) — shop domain: customers, quotes, invoices, products, pricing.
- **Delivery shells** — binaries that compose engine + application for different deployment targets:
  - `apps/mokumo-desktop/` — Tauri binary (desktop window + embedded web UI)
  - `apps/mokumo-server/` — **Headless binary** (Linux/container, zero Tauri deps — invariant I3, CI-enforced)
  - `services/api/src/main.rs` — Transitional CLI binary (will retire in Stage 8, folded into mokumo-server)

The shared router-building library is `services/api/` (crate name: `mokumo-api`). It provides `build_app_with_shutdown()` which assembles the full Axum router with auth, session, middleware, and routes. Both delivery shells depend on it.

### What we just built (PR #600, issues #508/#510)

We implemented the `mokumo-server` headless binary with:
- Clap dispatch: `serve` (TCP data plane + Unix domain socket admin surface), `diagnose`, `bootstrap`, `backup`
- `kikan-socket::serve_unix_socket()` — Axum router over UDS with mode 0600 fs-permission auth
- `mokumo-api::admin_uds::build_admin_uds_router()` — control-plane endpoints without session middleware
- `kikan-cli::UdsClient` — HTTP-over-Unix-socket client for CLI→daemon dispatch
- 4 UDS integration tests

---

## The Problem

CI failed on the `kikan-musl-build` job:

```
cargo build -p mokumo-server --release --target x86_64-unknown-linux-musl
```

Error:
```
error[E0599]: no function or associated item named `get` found for struct `SpaAssets`
  --> services/api/src/lib.rs:1723:44
     |
 248 | struct SpaAssets;
     | ---------------- function or associated item `get` not found for this struct
...
1723 |     } else if let Some(index) = SpaAssets::get("index.html") {
     |                                            ^^^ function or associated item not found in `SpaAssets`
```

### Root cause

`mokumo-api/src/lib.rs` contains:

```rust
#[derive(Embed)]       // from rust-embed crate
#[folder = "../../apps/web/build"]
struct SpaAssets;
```

This embeds the SvelteKit SPA (static HTML/JS/CSS) into the binary. In release mode, `rust-embed` reads the folder at **compile time** and generates `get()`, `iter()`, etc. methods. The `apps/web/build/` directory is gitignored — it only exists after `moon run web:build`. The musl CI job builds `mokumo-server` without running `web:build`, so the folder doesn't exist, and `rust-embed`'s derive macro fails.

`mokumo-server` is a headless binary — it should never serve the SPA. But because it depends on `mokumo-api` (for the shared router-building logic), it transitively compiles the SPA embedding code.

### Current architecture of the SPA attachment

The router assembly is already partially correct:

1. `build_app_inner()` returns the router **without** `.fallback(serve_spa)`.
2. Each caller independently attaches the SPA fallback:
   - `services/api/src/main.rs`: `app.fallback(mokumo_api::serve_spa)` ← transitional binary
   - `apps/mokumo-desktop/src/lib.rs`: `router.fallback(mokumo_api::serve_spa)` ← desktop shell
   - `apps/mokumo-server/src/main.rs`: passes `router` directly to `axum::serve` with **no** fallback ← headless shell (correct!)

So the **fallback attachment** is already separated. The problem is that `SpaAssets` (the embed struct) and `serve_spa` (the handler function) still compile unconditionally as part of `mokumo-api`, pulling in `rust-embed` as a hard dependency.

---

## Options Considered

### Option 1: Feature flag on mokumo-api

Add `spa = ["dep:rust-embed"]` feature to `mokumo-api`. Gate `SpaAssets`, `serve_spa`, `spa_response`, and the `rust_embed::Embed` import behind `#[cfg(feature = "spa")]`. Set `default = ["spa"]` so existing consumers get it automatically. `mokumo-server` uses `default-features = false`.

**Pros:**
- Smallest diff. No new crates, no duplication.
- Completes the pattern already 80% in place (fallback attachment is already caller-controlled; this gates the remaining compile-time artifact).
- The gate is narrow and sharp: 1 import, 1 struct, 2 functions, 2 tests.
- Stage 8 compatible: when `services/api/src/main.rs` retires, only `mokumo-desktop` and `mokumo-server` remain. The feature continues to work unchanged.
- I4 (DAG) unaffected — no new dependency edges. I5 (Tauri reachability) unaffected — `spa` has nothing to do with Tauri.
- `mokumo-server/Cargo.toml` already specifies `default-features = false` on its `mokumo-api` dep.

**Cons:**
- Conditional compilation in a library crate (but the surface area is minimal — 5 items behind one gate).

### Option 2: Move SPA embedding into each binary

Remove `SpaAssets`, `serve_spa`, `spa_response` from `mokumo-api/src/lib.rs`. Each binary that serves the SPA (`main.rs`, `mokumo-desktop`) defines its own `SpaAssets` and `serve_spa`.

**Pros:**
- `mokumo-api` becomes a pure logic library with no embed dependency.

**Cons:**
- Duplicates `serve_spa` (~47 lines) across two binaries. This function contains non-trivial shared logic:
  - API-path 404 guard: `/api/*` paths return JSON 404, not the SPA shell.
  - Cache-control policy: `/_app/immutable/` gets `max-age=31536000`, other assets get `max-age=3600`, `index.html` gets `no-cache`.
  - Error response formatting using `kikan_types::error::ErrorBody`.
- Duplicating this creates a divergence risk. Changes to caching policy or the API-path guard would need to be applied in two places.
- The `build_app` test-only wrapper currently calls `.fallback(serve_spa)` — tests would need to depend on a binary crate for the function, or the test helper would need restructuring.

### Option 3: Split mokumo-api into two crates

Create `mokumo-api-core` (router building, no SPA) and `mokumo-api` (re-exports core + SPA embedding).

**Pros:**
- Cleanest architectural boundary at the crate level.

**Cons:**
- Disproportionate for 60 lines of gated code.
- Doubles the crate count in `services/api/`, complicates the workspace.
- Creates a re-export layer that adds indirection.
- Stage 8 changes the consumer picture — needs re-assessment then.

### Option 4: Extract SPA into its own tiny crate (e.g. `mokumo-spa`)

Move `SpaAssets`, `serve_spa`, `spa_response` into a dedicated `mokumo-spa` crate.

**Pros:**
- No duplication of `serve_spa` logic.
- `mokumo-api` drops the `rust-embed` dependency entirely.

**Cons:**
- Creates a new crate for ~60 lines of code.
- `serve_spa` uses `kikan_types::error::ErrorBody` and `ErrorCode`, so `mokumo-spa` would need a dependency on `kikan-types` — pulling it beyond a "just embed files" crate.
- Unclear ownership: it's not engine, not application, not really a "service" — it's a delivery concern.
- Same Stage 8 re-assessment issue.

### Option 5: Make `build_app_inner` take an optional fallback handler

**This doesn't solve the problem.** `build_app_inner` already returns the router without the SPA fallback. The callers already attach it independently. The issue is compile-time: `SpaAssets` and `serve_spa` still compile unconditionally as part of `mokumo-api`, even though no headless code calls them.

---

## Recommendation

**Option 1: Feature flag (`spa` on mokumo-api).**

### Why

1. **SPA embedding is a delivery-shell concern.** The `spa` feature semantically means "this delivery shell serves a web frontend." The headless server shell does not; the desktop shell does. Feature flags on a shared library are the standard Rust mechanism for this — analogous to `serde`'s `derive` feature or `tokio`'s `full` feature.

2. **Completes an existing pattern.** The fallback attachment point is already caller-controlled. The feature flag gates the last unconditionally-compiled artifact (`SpaAssets` derive + `serve_spa` function body). This is not introducing conditional compilation into a clean library — it's finishing a separation that's 80% done.

3. **Proportionate response.** The gate covers 5 items (~60 lines). Creating a new crate or duplicating logic across binaries for 60 lines of code is architecturally over-engineered.

4. **No divergence risk.** `serve_spa` stays in one place. Cache-control policy, API-path guard, and error formatting evolve in one location. All consumers that want the SPA get the same implementation.

5. **Zero rework at Stage 8.** When the transitional binary retires, the feature continues to serve `mokumo-desktop` vs `mokumo-server` with no changes.

6. **DAG-clean.** No new dependency edges. I4 (one-way DAG), I5 (Tauri reachability) unaffected.

### Implementation

```toml
# services/api/Cargo.toml
[features]
default = ["spa"]
spa = ["dep:rust-embed"]
bdd = []

[dependencies]
rust-embed = { workspace = true, optional = true }
```

```rust
// services/api/src/lib.rs — gate these 5 items:

#[cfg(feature = "spa")]
use rust_embed::Embed;

#[cfg(feature = "spa")]
#[derive(Embed)]
#[folder = "../../apps/web/build"]
pub struct SpaAssets;

#[cfg(feature = "spa")]
fn spa_response(...) -> Response { ... }

#[cfg(feature = "spa")]
pub async fn serve_spa(uri: axum::http::Uri) -> Response { ... }

// tests:
#[cfg(feature = "spa")]
#[tokio::test]
async fn serve_spa_api_path_returns_not_found_code() { ... }

#[cfg(feature = "spa")]
#[tokio::test]
async fn serve_spa_prefix_collision_not_caught_by_api_guard() { ... }
```

```toml
# apps/mokumo-server/Cargo.toml — already in place:
mokumo-api = { path = "../../services/api", default-features = false }
```

Callers that want SPA (desktop, transitional main.rs) use default features and call `.fallback(mokumo_api::serve_spa)` after `build_app_with_shutdown`. The headless binary does not.

---

## What We Need From the Reviewer

1. Do you agree that Option 1 (feature flag) is the correct approach, or do you see an architectural concern we missed?
2. Is there a better pattern for compile-time gating of embedded assets in a shared Rust library?
3. Any concerns about the `default = ["spa"]` approach (all consumers get SPA by default, headless opts out)?
4. Would you recommend a different option, and if so, why?
