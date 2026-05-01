# scorecard

The scorecard crate owns the typed wire format for the **sticky PR scorecard**
comment that mokumo posts on every pull request. It is the source of truth
for both producer and renderer: a Rust binary writes `scorecard.json` from
gate outputs, the committed JSON Schema in `.config/scorecard/schema.json`
is the validator's contract, and a Node renderer in
`.github/workflows/scorecard-comment.yml` reads the artifact and posts/updates
the sticky comment.

The crate is governed by the ADR
`decisions/mokumo/adr-scorecard-crate-shape.md` (in the `ops` repo —
this product repo links to it via the `pipelines/mokumo/` workflow,
not via a relative path). The ADR's four-forcing-functions framework chose a
**single crate** over a multi-crate split; the lib stays mokumo-deps-zero
(`serde + schemars + serde_json` only) and binary tooling lives behind
the optional `cli` feature so downstream consumers don't transitively pull
`toml`, `walkdir`, `clap`, or `jsonschema`. Verify with
`cargo tree -p scorecard --no-default-features`.

## Three layers of `failure_detail_md` enforcement

A row with `status: Red` MUST carry an inline `failure_detail_md` string.
This is enforced at three layers (ADR §"Three layers"):

| Layer | Mechanism | Where |
|---|---|---|
| 1 — Typestate | `Row` and each variant are `#[non_exhaustive]`, so external callers cannot struct-literal a row and cannot match without a wildcard arm. They must construct via methods on `Row`; the methods mint `Status` internally so callers cannot supply a wrong status; `Row::coverage_delta_red` takes `failure_detail_md: String` (not `Option<String>`) | `src/lib.rs` |
| 2 — JSON Schema | `if status == "Red" then required: ["failure_detail_md"], properties.failure_detail_md.type: "string"` injected by post-processing — closes both the missing-field and the explicit-null paths. Helper lives in `src/schema_postprocess.rs`, consumed by both the binary and the drift-check integration test | `src/schema_postprocess.rs` |
| 3 — Renderer defensive | Renderer logs warning + renders `(detail missing — see workflow logs)` if a Red row arrives without detail | `.github/workflows/scorecard-comment.yml` |

Layer 1 is verified by the `tests/trybuild/` UI tests. Layer 2 is verified
by the `schema_drift` integration test, which asserts the binary output
matches the committed schema byte-for-byte (any change to the if/then
helper that breaks injection will fail this test before merge).

## Trybuild ↔ rust-toolchain.toml coupling

`tests/trybuild/*.stderr` files are **rustc stderr snapshots** — trybuild
compares the actual compiler output to them byte-for-byte. That makes them
sensitive to rustc version: stable's six-week cadence drifts the wording
(note positions, help-line whitespace, error-code suggestions), which is
exactly the noise we don't want polluting unrelated PRs.

`rust-toolchain.toml` at the workspace root pins the toolchain. **Toolchain
bumps are a synchronized change** with the .stderr snapshots:

```bash
# 1. Bump rust-toolchain.toml
# 2. Regenerate the snapshots:
TRYBUILD=overwrite cargo test -p scorecard --test trybuild
# 3. Verify the tests still fail for the right reason:
cargo test -p scorecard --test trybuild
# 4. Inspect the diffs in tests/trybuild/*.stderr and confirm each test
#    still asserts the same error code (E0061 for arity, E0308 for type
#    mismatch, E0639 for non_exhaustive).
```

If the .stderr drift is more than whitespace, audit it before accepting —
the tests are supposed to catch typestate violations, not "compiler
reformatted its output".

## Regenerating the committed schema

```bash
cargo run -p scorecard --bin emit-schema -- --out .config/scorecard/schema.json
```

`emit-schema` uses only the lib's deps (no `--features cli` needed) so a
drift-check workflow can regenerate the schema cheaply. The integration
test `tests/schema_drift.rs` enforces byte-identity between the binary's
output and the committed file on every `cargo test` run.

## Vendored ajv bundle

The renderer in `.github/workflows/scorecard-comment.yml` validates the
scorecard artifact against `.config/scorecard/schema.json` via a
**vendored** ajv bundle at `.github/scripts/scorecard/ajv-bundle.js`.
Bundling (with esbuild `--platform=node --format=cjs`) keeps the
renderer step network-free at CI runtime and pins the validator to an
exact version reviewed at regen-time.

The bundle is committed verbatim alongside a `.VERSIONS` sidecar that
captures the pinned ajv version, the bundle date, and the bundler tag
so any operator can audit the supply chain without rebuilding.

### Regenerate the bundle

```bash
tools/update-vendored-ajv.sh
```

The script:

1. Installs the pinned ajv version (currently `8.16.0`) into a temp dir.
2. Runs `esbuild` with `--bundle --platform=node --format=cjs` to
   produce a single CommonJS file the renderer can `require()`.
3. Writes `.github/scripts/scorecard/ajv-bundle.js` and
   `.github/scripts/scorecard/.VERSIONS`.

To bump the pinned version, edit `AJV_VERSION` at the top of
`tools/update-vendored-ajv.sh`, run the script, review the diff, and
commit both the new bundle and the updated `.VERSIONS` sidecar in the
same PR.

### Cadence

Per ADR `decisions/mokumo/adr-scorecard-crate-shape.md`, the vendored
bundle is refreshed on a **quarterly cadence** (or sooner for security
advisories affecting ajv). Open a tracking issue at the start of each
quarter to regenerate; the regen PR also bumps `.VERSIONS` and (if
ajv published a new minor) updates `AJV_VERSION` in the script.

The renderer's vitest suite at
`.github/scripts/scorecard/__tests__/` exercises the validator + the
sticky-comment poster against the committed schema, so any regression
in the bundled ajv API surface fails CI before merge.

## Renderer types — `.github/scripts/scorecard/types.d.ts`

The renderer is plain CommonJS JavaScript, but its public-API surface is
locked to the schema by `// @ts-check` headers and JSDoc `@param` /
`@returns` annotations referencing `./types`. The `types.d.ts` file is
**generated** from the committed JSON Schema by
[`json-schema-to-typescript`](https://github.com/bcherny/json-schema-to-typescript)
and committed verbatim alongside `render.js` + `validate.js`.

This is the third layer of producer/renderer wire-shape locking,
complementing Layer 1 (typestate-by-construction in the producer crate)
and Layer 2 (`if/then` schema invariant on Red rows enforced by ajv at
render time).

### Regenerate locally

```bash
tools/regen-types.sh
```

The script runs the two regenerations in sequence:

1. `cargo run -p scorecard --bin emit-schema -- --out .config/scorecard/schema.json`
   — emits the canonical schema from `schemars` derive output.
2. `pnpm dlx json-schema-to-typescript@<pinned> .config/scorecard/schema.json --output .github/scripts/scorecard/types.d.ts`
   — projects the schema into TypeScript declarations.

Run before pushing if you've touched the Rust scorecard types or
edited the schema by hand. The renderer-side `pnpm typecheck`
(invokes `tsc --noEmit` over `render.js` + `validate.js`) catches
JSDoc/types desync.

### CI gate

The `scorecard-drift` job in `.github/workflows/quality.yml` runs the
same two regenerations on every PR and fails on any uncommitted diff.
Together with the in-process `tests/schema_drift.rs` byte-identity test
this closes four desync paths:

| Desync | Gate that catches it |
|---|---|
| Rust source change without schema regen | `schema_drift.rs` (in-process) |
| Hand-edit `.config/scorecard/schema.json` | `scorecard-drift` (also `schema_drift.rs`) |
| Hand-edit `.github/scripts/scorecard/types.d.ts` | `scorecard-drift` only (V2's new capability) |
| JSDoc disagrees with `types.d.ts` | `scorecard-drift` `tsc --noEmit` step |

The `json-schema-to-typescript` and `typescript` versions are pinned
exactly in `.github/scripts/scorecard/package.json` so `git diff
--exit-code` on the regenerated `types.d.ts` is stable across runners.
Bumps to either pin go through the same regen-and-commit flow as the
ajv refresh above.
