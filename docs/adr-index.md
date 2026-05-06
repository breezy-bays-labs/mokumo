# ADR Index

Auto-generated registry of in-repo Architecture Decision Records that opt into
the `enforced-by:` frontmatter contract. The body of this file between the
markers below is owned by `tools/docs-gen` (see
[`tools/docs-gen/src/registry.rs`](../tools/docs-gen/src/registry.rs)) and is
rewritten by `moon run docs:gen`. Do not edit between the markers by hand.

The companion ADR vault lives in the private ops repo at
`ops/decisions/mokumo/`; entries there reach the index only when their
counterpart lands in `docs/adr/` with the YAML frontmatter and `enforced-by:`
contract. Schema and `enforced-by:` semantics: see
[`AGENTS.md`](../AGENTS.md#synchronized-docs).

<!-- AUTO-GEN:adr-index -->
| Title | Status | Source | Enforcement |
|---|---|---|---|
| ADR: Handler ↔ Scenario Coverage Gate (Per-Route BDD-Coverage Axis) | approved | [docs/adr/adr-handler-scenario-coverage.md](docs/adr/adr-handler-scenario-coverage.md) | lint, test, workflow |
| ADR: Public-API Spec Audit Gate (BDD coverage of pub items) | approved | [docs/adr/adr-pub-api-spec-audit.md](docs/adr/adr-pub-api-spec-audit.md) | lint, test, workflow |
<!-- /AUTO-GEN:adr-index -->
