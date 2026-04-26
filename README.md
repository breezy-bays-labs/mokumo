# Mokumo

[![Quality Loop](https://github.com/breezy-bays-labs/mokumo/actions/workflows/quality.yml/badge.svg)](https://github.com/breezy-bays-labs/mokumo/actions/workflows/quality.yml)
[![License: BUSL-1.1](https://img.shields.io/badge/License-BUSL--1.1-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-Axum-orange.svg)](https://github.com/tokio-rs/axum)
[![SvelteKit](https://img.shields.io/badge/SvelteKit-Svelte%205-ff3e00.svg)](https://svelte.dev)

> **Status: Pre-alpha** — Active development. Not yet ready for production use.

Production management software for decorated apparel shops. Full garment lifecycle: Quote, Artwork Approval, Production, Shipping, Invoice.

## Architecture

Mokumo is a vertical application built on **Kikan** — a self-hosted Rust application platform that lives in the same repo. Two binaries ship from one workspace:

- **`apps/mokumo-desktop`** — Tauri v2 native desktop bundle for non-technical shop owners. Spawns the Axum server in-process; webview points at `localhost`.
- **`apps/mokumo-server`** — Headless Axum binary for self-hosters running on a NAS, VM, container, or Tailscale node. Tauri-free (CI-enforced); admin operations reach it over a Unix domain socket via the `kikan-cli` tool.

Stack:

- **Frontend**: SvelteKit (Svelte 5 runes) + Tailwind v4 + shadcn-svelte, compiled to a static SPA via `adapter-static`.
- **Backend**: Rust (Axum) — Kikan engine + Mokumo vertical, sharing one router between desktop, headless, and LAN clients.
- **Database**: Multi-tenant SQLite — Kikan-owned `meta.db` (platform-wide) + per-profile `mokumo.db` (vertical data). SeaORM 2.0 + SQLx with backup-before-migrate safety.
- **Type sharing**: Rust DTOs in `crates/kikan-types` derive `ts-rs` to auto-generate TypeScript bindings for the frontend.
- **LAN access**: mDNS discovery for browser clients (`{shop}.local`); deployment-mode middleware adapts cookie flags / CSRF / rate limiting per trust posture.
- **Monorepo**: Moon orchestrates Rust + Node toolchains.

**For the full system design** — crate map, Graft/SubGraft pattern, control plane vs data plane, upgrade safety, quality invariants, decision index — see [`ARCHITECTURE.md`](ARCHITECTURE.md). For the threat model and deployment-mode trust boundaries, see [`SECURITY.md`](SECURITY.md). For working in the repo, see [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Project Structure

```
mokumo/
├── apps/
│   ├── mokumo-desktop/   # Tauri v2 desktop binary
│   ├── mokumo-server/    # Headless binary (zero Tauri deps — invariant I3)
│   └── web/              # SvelteKit frontend (adapter-static)
├── crates/
│   ├── kikan/            # Engine — tenancy, migrations, auth, control plane
│   ├── kikan-types/      # Wire DTOs (serde + ts-rs)
│   ├── kikan-tauri/      # Tauri-shell helpers (no tauri:: in kikan public API)
│   ├── kikan-cli/        # Admin CLI library (UDS HTTP client)
│   ├── kikan-socket/         # Unix domain socket primitives
│   ├── kikan-spa-sveltekit/  # SvelteKit SpaSource impls (embedded + disk)
│   ├── kikan-events/         # Event bus SubGraft
│   ├── kikan-mail/           # Mailer SubGraft (lettre)
│   ├── kikan-scheduler/      # Job scheduler SubGraft (apalis)
│   └── mokumo-shop/          # Shop vertical (customers, quotes, kanban, invoices)
├── docs/
│   ├── adr/              # Architecture decision records (security headers, ...)
│   └── diagrams/         # D2 source + rendered SVGs (see ARCHITECTURE.md)
├── scripts/              # CI invariant checks (I1–I5)
└── tools/
    └── license-server/   # License validation
```

The dependency graph between these crates is enforced by CI invariant I4 — see [`ARCHITECTURE.md` §2](ARCHITECTURE.md#2-workspace-crate-map) for the canonical edge map.

## Development

Prerequisites: Rust, Node.js 22+, pnpm, [Moon](https://moonrepo.dev), [D2](https://d2lang.com) (for architecture diagrams).

```bash
pnpm install                  # Install JS dependencies
moon run shop:db-prepare      # Prepare SQLx offline cache
moon check --all              # Full CI matrix locally
moon run web:dev              # SvelteKit dev server
moon run shop:dev             # Axum backend with auto-reload
```

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full toolchain, day-to-day commands, branching/commit conventions, and quality gates.

## Getting Started

Mokumo is a self-hosted desktop application — download it, run it, and own your data.

### What you'll need

- A computer running **Windows 10+** or **macOS 12+**
- A second device on the same WiFi network (for the LAN demo)

### Download

Grab the latest installer from [GitHub Releases](https://github.com/breezy-bays-labs/mokumo/releases).

### Platform notes

- **Windows**: SmartScreen may warn about an unrecognized publisher. Click **More info → Run anyway** to proceed.
- **macOS**: Gatekeeper may block the app. Right-click the app and select **Open**, then confirm in the dialog.

### Demo Guide

Follow the interactive walkthrough to set up your shop and explore every feature:

**[Open the Demo Guide →](https://breezy-bays-labs.github.io/mokumo/)**

## License

[Business Source License 1.1](LICENSE) (BUSL-1.1). Free to use for your own shop. Cannot be offered as a competing hosted product. Converts to Apache 2.0 three years after each version's release.
