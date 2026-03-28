# Mokumo

[![Quality Loop](https://github.com/breezy-bays-labs/mokumo/actions/workflows/quality.yml/badge.svg)](https://github.com/breezy-bays-labs/mokumo/actions/workflows/quality.yml)
[![License: BUSL-1.1](https://img.shields.io/badge/License-BUSL--1.1-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-Axum-orange.svg)](https://github.com/tokio-rs/axum)
[![SvelteKit](https://img.shields.io/badge/SvelteKit-Svelte%205-ff3e00.svg)](https://svelte.dev)

> **Status: Pre-alpha** — Active development. Not yet ready for production use.

Production management software for decorated apparel shops. Full garment lifecycle: Quote, Artwork Approval, Production, Shipping, Invoice.

## Architecture

Self-hosted SvelteKit + Rust (Axum) binary. Shops download, run, and own their data.

- **Frontend**: SvelteKit (Svelte 5) + Tailwind v4 + shadcn-svelte, compiled to static SPA
- **Backend**: Rust (Axum) API server with embedded SPA via rust-embed
- **Database**: SQLite (embedded, per-shop) with SeaORM + SQLx
- **Desktop**: Tauri v2 wraps the server into a native application
- **LAN access**: mDNS discovery for browser clients on local network
- **Monorepo**: Moon orchestrates both Rust and Node toolchains

## Project Structure

```
mokumo/
├── apps/
│   ├── desktop/       # Tauri v2 desktop shell
│   └── web/           # SvelteKit frontend (adapter-static)
├── services/
│   └── api/           # Axum backend
├── crates/
│   ├── core/          # Domain logic (pure Rust, no framework deps)
│   ├── types/         # API DTOs with ts-rs for TypeScript generation
│   └── db/            # SeaORM entities + repository implementations
└── tools/
    └── license-server/ # License validation
```

## Development

Prerequisites: Rust, Node.js 22+, pnpm, [Moon](https://moonrepo.dev)

```bash
pnpm install                  # Install JS dependencies
moon run web:dev              # SvelteKit dev server
moon run api:dev              # Axum backend with auto-reload
moon run api:test             # Backend tests
moon run web:test             # Frontend tests
moon check --all              # Full CI suite
```

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
