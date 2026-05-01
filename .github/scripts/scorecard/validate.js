// @ts-check
// Scorecard schema validator. Wraps the vendored ajv bundle.
//
// Two surfaces:
//   - Library: `validateScorecard(schema, data)` returns
//     `{ valid: true }` or `{ valid: false, pointer, value, message }`.
//     Used by the renderer workflow + vitest tests.
//   - CLI:     `node validate.js <schema-path> <data-path>` reads both
//     files, runs the validator, prints the JSON Pointer + offending
//     value on failure, exits 0 on valid / 2 on invalid / 1 on I/O error.
//
// The schema enforces the Layer 2 `if status == "Red" then required:
// failure_detail_md` invariant. We turn ajv's first error into a
// JSON Pointer + value pair that the renderer can surface in the
// fail-closed sticky comment.

"use strict";

const fs = require("node:fs");

/** @type {any} */
const Ajv = require("./ajv-bundle.js").default;

/** Build an Ajv instance configured for the scorecard schema. */
function makeAjv() {
  // strict: false — schemars 0.8 emits draft-07 with constructs that
  // ajv's strict mode flags as ambiguous (e.g. `allOf` wrapping a
  // `$ref`). The schema is the trust boundary, not ajv's strict-mode
  // opinions; the producer's drift-check guarantees byte-identity to
  // the Rust source.
  return new Ajv({ allErrors: false, strict: false });
}

/** Walk a JSON value by JSON Pointer (RFC 6901). Returns `undefined` if
 *  the pointer doesn't resolve.
 *
 *  @param {unknown} data
 *  @param {string | null | undefined} pointer
 *  @returns {unknown}
 */
function resolvePointer(data, pointer) {
  if (pointer === "" || pointer == null) return data;
  const parts = pointer
    .replace(/^\//, "")
    .split("/")
    .map((p) => p.replace(/~1/g, "/").replace(/~0/g, "~"));
  /** @type {any} */
  let cur = data;
  for (const p of parts) {
    if (cur == null) return undefined;
    cur = Array.isArray(cur) ? cur[Number(p)] : cur[p];
  }
  return cur;
}

/** Validate `data` against `schema`. Returns a structured result.
 *
 *  @param {object} schema
 *  @param {unknown} data
 *  @returns {ValidationResult}
 */
function validateScorecard(schema, data) {
  const ajv = makeAjv();
  const validate = ajv.compile(schema);
  const ok = validate(data);
  if (ok) return { valid: true };

  const err = (validate.errors && validate.errors[0]) || {};
  const pointer = err.instancePath || "";
  const value = resolvePointer(data, pointer);
  const message = err.message || "validation failed";
  return {
    valid: false,
    pointer: pointer === "" ? "(root)" : pointer,
    value,
    message,
    keyword: err.keyword,
    schemaPath: err.schemaPath,
  };
}

/** CLI entry. Returns the exit code; the wrapper at the bottom hands
 *  it to `process.exit` so callers can `require()` this file from
 *  unit tests without aborting.
 *
 *  @param {string[]} argv
 *  @returns {number}
 */
function cliMain(argv) {
  if (argv.length !== 2) {
    process.stderr.write(
      "usage: node validate.js <schema-path> <data-path>\n",
    );
    return 1;
  }
  const [schemaPath, dataPath] = argv;
  /** @type {object} */
  let schema;
  /** @type {unknown} */
  let data;
  try {
    schema = JSON.parse(fs.readFileSync(schemaPath, "utf8"));
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    process.stderr.write(`validate.js: cannot read schema ${schemaPath}: ${msg}\n`);
    return 1;
  }
  try {
    data = JSON.parse(fs.readFileSync(dataPath, "utf8"));
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    process.stderr.write(`validate.js: cannot read data ${dataPath}: ${msg}\n`);
    return 1;
  }
  const result = validateScorecard(schema, data);
  if (result.valid) {
    process.stdout.write("ok\n");
    return 0;
  }
  process.stdout.write(JSON.stringify(result, null, 2) + "\n");
  return 2;
}

module.exports = { validateScorecard, resolvePointer, cliMain };

if (require.main === module) {
  process.exit(cliMain(process.argv.slice(2)));
}

/**
 * @typedef {{ valid: true } |
 *           { valid: false; pointer: string; value: unknown; message: string;
 *             keyword?: string; schemaPath?: string }} ValidationResult
 */
