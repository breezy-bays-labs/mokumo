# Mokumo Context

> Orientation hub for new agents and human contributors. Tells you which document answers which question. **This file does not try to teach you the architecture** — that's [`ARCHITECTURE.md`](ARCHITECTURE.md)'s job, and it does it better than a summary would. This file's job is "I have a question, where do I look?"
>
> If you're trying to absorb the system end-to-end, read [`ARCHITECTURE.md`](ARCHITECTURE.md) cover-to-cover. If you're trying to find a specific answer fast, use the map below.

---

## What this repo ships

Two products from one Cargo + pnpm workspace:

- **Kikan** — a self-hosted Rust application platform. Engine, tenancy, migrations, backup, auth, control plane. Headless-first. Knows nothing about decoration shops.
- **Mokumo** — a decorator garment management application. Quote → Artwork Approval → Production → Shipping → Invoice. The first vertical built on Kikan.

The architectural why and how are in [`ARCHITECTURE.md`](ARCHITECTURE.md).

---

## Where to look for X

| If you want to know… | Read… |
|---|---|
| **What the parts are and how they connect** (crates, dependency DAG, control / data plane, deployment topology, Graft trait, database layout, upgrade safety, invariants I1–I5, ADR index) | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| **What a term means** — `Engine`, `Graft`, `Profile`, `DeploymentMode`, `ActivityWriter` … | [`crates/kikan/LANGUAGE.md`](crates/kikan/LANGUAGE.md) (platform glossary) |
| **What a term means** — `Customer`, `Quote`, `Decoration Method`, `Garment`, `SetupMode` … | [`LANGUAGE.md`](LANGUAGE.md) (vertical glossary) |
| **Where vertical language meets platform language** (Profile, Active DB, Migration, User, Activity Log, Recovery Artifact, Setup Token) | The "Boundary terms" section at the bottom of either `LANGUAGE.md` |
| **How to set up the toolchain, run tests, ship a PR** | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
| **Day-to-day commands, conventions, gotchas** | [`CLAUDE.md`](CLAUDE.md) |
| **Per-crate conventions** | The crate's `AGENTS.md` (e.g. [`crates/kikan/AGENTS.md`](crates/kikan/AGENTS.md), [`crates/mokumo-shop/AGENTS.md`](crates/mokumo-shop/AGENTS.md)) |
| **How to report a vulnerability, what the threat model is, what's out of scope** | [`SECURITY.md`](SECURITY.md) |
| **Why a decision was made** | The relevant ADR in `ops/decisions/mokumo/` (private). [`ARCHITECTURE.md` §11](ARCHITECTURE.md#11-decision-index) carries the load-bearing Y-statement summaries when the link is dead. |
| **How synchronized docs and AUTO-GEN sections work** | [`AGENTS.md` §Synchronized-Docs](AGENTS.md#synchronized-docs) |

---

## First-time onboarding path

For an agent or human starting cold, in order:

1. **[`ARCHITECTURE.md`](ARCHITECTURE.md)** — read end-to-end. ~30 minutes. This is the real model of the system.
2. **[`crates/kikan/LANGUAGE.md`](crates/kikan/LANGUAGE.md)** and **[`LANGUAGE.md`](LANGUAGE.md)** — skim. You'll come back to look up specific terms; the goal of the first read is just to know what's in each.
3. **[`CLAUDE.md`](CLAUDE.md)** (or [`CONTRIBUTING.md`](CONTRIBUTING.md) for humans) — read the commands and conventions you'll use day-to-day.

Skip on a first read: per-crate `AGENTS.md` files (read them when you touch the crate); ADR text (the Y-statements in `ARCHITECTURE.md` §11 are usually enough); ops standards (private repo; the public docs cite the load-bearing ones inline).

---

## How the docs stay honest

Docs go stale when code changes and prose doesn't. Three mechanisms keep this set in sync:

- **`<!-- AUTO-GEN:* -->` markers** — sections of a doc owned by `tools/docs-gen` and overwritten on every run. The `docs-drift` CI gate fails if the regenerated content differs from HEAD. The marker registry is in [`AGENTS.md` §Synchronized-Docs](AGENTS.md#synchronized-docs).
- **Paired-files rules** — when a class of code changes, a specific doc must change in the same PR. Recorded in the same Synchronized-Docs section. CI enforcement of this rule is tracked in [issue #776](https://github.com/breezy-bays-labs/mokumo/issues/776).
- **Per-doc reality checks** — each `LANGUAGE.md` ends with a "When this glossary is wrong" note. ARCHITECTURE.md ends with a "How to update this document" note. The convention: the code wins, the doc gets fixed in the same PR, every time.

If you see a doc disagreeing with the code: the code wins. Fix the doc — in the same PR if you have one open, or open a new one.

---

## When in doubt

- **Architecture question** → [`ARCHITECTURE.md`](ARCHITECTURE.md)
- **What does this term mean?** → [`crates/kikan/LANGUAGE.md`](crates/kikan/LANGUAGE.md) or [`LANGUAGE.md`](LANGUAGE.md)
- **Day-to-day commands** → [`CLAUDE.md`](CLAUDE.md)
- **Per-crate conventions** → that crate's `AGENTS.md`
- **Security concern** → [`SECURITY.md`](SECURITY.md)
- **Why a decision was made** → ADRs in `ops/decisions/mokumo/` (private; Y-statements in [`ARCHITECTURE.md` §11](ARCHITECTURE.md#11-decision-index))

If none of those answers your question, the answer probably belongs in one of the documents above. Open a discussion or DM the maintainer; we'd rather extend the doc than have you guess.
