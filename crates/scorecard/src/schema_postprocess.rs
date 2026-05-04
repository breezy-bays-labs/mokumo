//! Layer 2 schema post-processing — single source of truth for the
//! `if/then` injection that closes the `failure_detail_md` invariant.
//!
//! schemars cannot natively express "if `status == Red` then
//! `failure_detail_md` is required and non-null" as a derive attribute.
//! [`inject_red_requires_detail`] walks the derived schema's `Row.oneOf`
//! array (one entry per tagged-union variant), verifies every variant is
//! capable of going Red, and injects the conditional invariant per
//! variant.
//!
//! Both the `emit-schema` binary and the `schema_drift` integration test
//! call this helper. The committed `.config/scorecard/schema.json` is the
//! byte-for-byte output of `schema_for!(Scorecard)` followed by
//! `inject_red_requires_detail`.
//!
//! `pub` is required because integration tests are external from the
//! lib's perspective; `#[doc(hidden)]` on the module keeps the helpers
//! out of the rustdoc surface so they aren't taken as public API by
//! downstream consumers.

#![doc(hidden)]

use schemars::schema::{RootSchema, Schema, SchemaObject, StringValidation};
use serde_json::json;

/// Pattern enforced on every URL-typed field in the schema. The
/// renderer's "two-click rule" links MUST be HTTPS; the pattern is the
/// last line of defense if a producer accidentally emits an `http://`
/// URL or a free-form string.
const HTTPS_URL_PATTERN: &str = "^https://";

/// Walk the `Row` definition's `oneOf` array and inject
/// `if status == Red then required: [failure_detail_md] && type: string`
/// on each variant subschema.
///
/// The helper enforces that *every* variant capable of going Red receives
/// the if/then guard. If schemars output shape changes such that some
/// variants are recognized and others are not (e.g. a future variant uses
/// a deeper `$ref` shape that `variant_defines_failure_detail` doesn't
/// reach), the equality assertion below fails loudly rather than silently
/// shipping unguarded variants to production.
///
/// # Panics
///
/// Panics on any unexpected schemars output shape — the helper is
/// build-time only, and a silent no-op would defeat Layer 2.
pub fn inject_red_requires_detail(schema: &mut RootSchema) {
    let Some(row_def) = schema.definitions.get_mut("Row") else {
        panic!(
            "scorecard::schema_postprocess: Row definition missing from schema; \
             schemars output may have changed shape (expected definitions[\"Row\"])"
        );
    };

    let Schema::Object(row_obj) = row_def else {
        panic!("scorecard::schema_postprocess: Row schema is a Bool, expected an Object");
    };

    let Some(subschemas) = row_obj.subschemas.as_mut() else {
        panic!("scorecard::schema_postprocess: Row schema has no subschemas; expected oneOf");
    };

    let Some(one_of) = subschemas.one_of.as_mut() else {
        panic!("scorecard::schema_postprocess: Row schema has no oneOf array");
    };

    let total_variants = one_of.len();
    let mut variants_patched = 0;
    for variant in one_of.iter_mut() {
        match variant {
            Schema::Object(variant_obj) => {
                assert!(
                    variant_defines_failure_detail(variant_obj),
                    "scorecard::schema_postprocess: Row variant does not define \
                     failure_detail_md; every Row variant must be capable of going \
                     Red. Either update the variant definition or update this helper \
                     to recognize the new shape."
                );
                inject_if_then_for_variant(variant_obj);
                variants_patched += 1;
            }
            Schema::Bool(_) => panic!(
                "scorecard::schema_postprocess: Row variant is Schema::Bool, expected \
                 Object; schemars output shape changed"
            ),
        }
    }

    assert_eq!(
        variants_patched, total_variants,
        "scorecard::schema_postprocess: only {variants_patched} of {total_variants} \
         Row variants received the if/then guard. Layer 2 would silently fail to enforce \
         failure_detail_md on the unpatched variants. Audit schemars output and update \
         inject_red_requires_detail."
    );
}

/// Return true if this variant subschema declares a `failure_detail_md`
/// property (either directly or transitively via flattened `RowCommon`).
fn variant_defines_failure_detail(obj: &SchemaObject) -> bool {
    if obj
        .object
        .as_ref()
        .is_some_and(|o| o.properties.contains_key("failure_detail_md"))
    {
        return true;
    }
    // schemars emits `#[serde(flatten)] common: RowCommon` as `allOf` of
    // the variant's own properties + a `$ref` to RowCommon. Recurse into
    // any nested allOf entries to catch variants that move the field onto
    // a flattened struct.
    if let Some(subs) = obj.subschemas.as_ref()
        && let Some(all_of) = subs.all_of.as_ref()
    {
        for s in all_of {
            if let Schema::Object(inner) = s
                && variant_defines_failure_detail(inner)
            {
                return true;
            }
        }
    }
    false
}

/// Inject `if: { required: ["status"], properties: { status: { const: "Red" } } },
/// then: { required: ["failure_detail_md"], properties: { failure_detail_md:
/// { type: "string" } } }` into the variant subschema.
///
/// Three things matter about the shape:
///
/// - `if.required: ["status"]` — without it, a payload that omits
///   `status` entirely would vacuously satisfy `if` and trigger `then`.
///   Defense in depth; the outer schema requires `status` already.
/// - `then.required: ["failure_detail_md"]` — JSON Schema's `required`
///   is key presence, not value, so the next constraint is also
///   load-bearing.
/// - `then.properties.failure_detail_md.type: "string"` — overrides the
///   schemars-default `["string", "null"]` so a producer cannot emit
///   `failure_detail_md: null` and still pass validation.
///
/// `SubschemaValidation` exposes typed `if_schema` / `then_schema` fields
/// — schemars 0.8's first-class slot for the JSON Schema `if`/`then`
/// keywords. The if/then bodies are deserialized from `serde_json::Value`
/// literals (per-variant, build-time only) to keep the call site
/// readable and avoid hand-constructing nested `SchemaObject` chains.
fn inject_if_then_for_variant(variant: &mut SchemaObject) {
    let if_schema: Schema = serde_json::from_value(json!({
        "required": ["status"],
        "properties": {
            "status": { "const": "Red" }
        }
    }))
    .expect("scorecard::schema_postprocess: if_schema literal");
    let then_schema: Schema = serde_json::from_value(json!({
        "required": ["failure_detail_md"],
        "properties": {
            "failure_detail_md": { "type": "string" }
        }
    }))
    .expect("scorecard::schema_postprocess: then_schema literal");

    let subschemas = variant.subschemas.get_or_insert_with(Default::default);
    subschemas.if_schema = Some(Box::new(if_schema));
    subschemas.then_schema = Some(Box::new(then_schema));
}

/// Tighten URL-shaped fields in the schema with `format: "uri"` and
/// `pattern: "^https://"`. Closes the gap that the field's prose
/// description ("Absolute https:// URL...") was the only enforcement —
/// validators ignore prose, so a producer regression could ship a
/// `http://` URL through the schema and the renderer would link it
/// without complaint.
///
/// # Panics
///
/// Panics on any unexpected schemars output shape. Build-time only.
pub fn tighten_url_fields(schema: &mut RootSchema) {
    tighten_string_property(&mut schema.schema, "all_check_runs_url");

    let Some(gate_run) = schema.definitions.get_mut("GateRun") else {
        panic!(
            "scorecard::schema_postprocess: GateRun definition missing; schemars \
             output may have changed shape (expected definitions[\"GateRun\"])"
        );
    };
    let Schema::Object(gate_run_obj) = gate_run else {
        panic!("scorecard::schema_postprocess: GateRun schema is a Bool, expected Object");
    };
    tighten_string_property(gate_run_obj, "url");
}

/// Strip non-standard JSON Schema `format` annotations from numeric
/// properties throughout the schema.
///
/// schemars annotates `f64` with `format: "double"`, `u32` with
/// `format: "uint32"`, and similar for other numeric types. These are
/// schemars idioms, not draft-07 standard formats, and ajv emits a
/// warning + degraded validation when it encounters them. Removing the
/// hint is harmless for our schemas: numeric range constraints are
/// expressed via `minimum` / `maximum` keywords (which ajv supports),
/// not via the format hint.
///
/// String formats (`uri`, `date-time`, ...) are NOT stripped — those
/// are standard JSON Schema and ajv understands them.
///
/// Used by the operator-facing schema (`quality.config.schema.json`)
/// which ajv-cli validates the committed `quality.toml` against. The
/// wire schema (`schema.json`) is validated by the `jsonschema` crate
/// in the producer, which silently ignores unknown formats, so the
/// strip is unnecessary there — and intentionally not applied — to
/// preserve the format hints as type-discovery aids for hand-readers.
pub fn strip_nonstandard_number_formats(schema: &mut RootSchema) {
    strip_nonstandard_number_formats_in(&mut schema.schema);
    for value in schema.definitions.values_mut() {
        if let Schema::Object(obj) = value {
            strip_nonstandard_number_formats_in(obj);
        }
    }
}

fn strip_nonstandard_number_formats_in(obj: &mut SchemaObject) {
    strip_format_if_nonstandard_numeric(obj);
    recurse_into_object_properties(obj);
    recurse_into_subschema_branches(obj);
}

/// Returns true when this schema's `instance_type` is a single
/// numeric type (`Number` or `Integer`). Composite/array types are
/// left alone — the strip targets schemars' default per-numeric-leaf
/// `format` annotation, not synthetic union types.
fn is_single_numeric_type(obj: &SchemaObject) -> bool {
    use schemars::schema::{InstanceType, SingleOrVec};
    let Some(SingleOrVec::Single(boxed)) = obj.instance_type.as_ref() else {
        return false;
    };
    matches!(**boxed, InstanceType::Number | InstanceType::Integer)
}

/// Drop the `format` field on this schema when it carries a schemars
/// numeric-type idiom (`double`, `uint32`, …). Standard string formats
/// listed in [`STANDARD_STRING_FORMATS`] are preserved unconditionally
/// so a string-typed leaf with `format: "uri"` is never disturbed by
/// this pass.
fn strip_format_if_nonstandard_numeric(obj: &mut SchemaObject) {
    if !is_single_numeric_type(obj) {
        return;
    }
    let Some(format) = obj.format.as_deref() else {
        return;
    };
    if STANDARD_STRING_FORMATS.contains(&format) {
        return;
    }
    obj.format = None;
}

/// Recurse into `obj.properties.*` and run the strip on each child
/// `SchemaObject`. `Schema::Bool` properties are skipped — they carry
/// no `format` field and nothing to recurse into.
fn recurse_into_object_properties(obj: &mut SchemaObject) {
    let Some(object_validation) = obj.object.as_mut() else {
        return;
    };
    for prop in object_validation.properties.values_mut() {
        if let Schema::Object(child) = prop {
            strip_nonstandard_number_formats_in(child);
        }
    }
}

/// Recurse into `obj.subschemas.{all_of, one_of, any_of}` and run the
/// strip on every `Schema::Object` branch. The three keywords are
/// walked uniformly via [`recurse_into_subschema_branch_list`]; a new
/// subschema keyword would just need one more call site here.
fn recurse_into_subschema_branches(obj: &mut SchemaObject) {
    let Some(subs) = obj.subschemas.as_mut() else {
        return;
    };
    recurse_into_subschema_branch_list(subs.all_of.as_mut());
    recurse_into_subschema_branch_list(subs.one_of.as_mut());
    recurse_into_subschema_branch_list(subs.any_of.as_mut());
}

/// Run the strip on every `Schema::Object` entry in a single subschema
/// branch list (i.e. one of `all_of`, `one_of`, `any_of`). `None` means
/// the keyword is absent from the parent; nothing to do.
fn recurse_into_subschema_branch_list(branches: Option<&mut Vec<Schema>>) {
    let Some(list) = branches else { return };
    for branch in list {
        if let Schema::Object(child) = branch {
            strip_nonstandard_number_formats_in(child);
        }
    }
}

/// JSON Schema draft-07 standard string formats that ajv recognizes by
/// default. Anything else on a numeric type is a schemars idiom we
/// strip from operator-facing schemas.
const STANDARD_STRING_FORMATS: &[&str] = &[
    "date-time",
    "time",
    "date",
    "email",
    "idn-email",
    "hostname",
    "idn-hostname",
    "ipv4",
    "ipv6",
    "uri",
    "uri-reference",
    "iri",
    "iri-reference",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];

/// Set `format: "uri"` and `pattern: HTTPS_URL_PATTERN` on `obj.properties[prop]`.
fn tighten_string_property(obj: &mut SchemaObject, prop: &str) {
    let Some(object_validation) = obj.object.as_mut() else {
        panic!(
            "scorecard::schema_postprocess: parent schema has no `properties` map; \
             cannot tighten {prop}"
        );
    };
    let Some(property) = object_validation.properties.get_mut(prop) else {
        panic!("scorecard::schema_postprocess: property {prop} missing on parent schema");
    };
    let Schema::Object(property_obj) = property else {
        panic!("scorecard::schema_postprocess: property {prop} is a Bool, expected Object");
    };
    property_obj.format = Some("uri".into());
    let string_validation = property_obj
        .string
        .get_or_insert_with(|| Box::new(StringValidation::default()));
    string_validation.pattern = Some(HTTPS_URL_PATTERN.into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema::{ObjectValidation, SubschemaValidation};

    fn schema_with_property(name: &str) -> SchemaObject {
        let mut obj = ObjectValidation::default();
        obj.properties
            .insert(name.into(), Schema::Object(SchemaObject::default()));
        SchemaObject {
            object: Some(Box::new(obj)),
            ..Default::default()
        }
    }

    fn schema_with_all_of(branches: Vec<Schema>) -> SchemaObject {
        let subs = SubschemaValidation {
            all_of: Some(branches),
            ..Default::default()
        };
        SchemaObject {
            subschemas: Some(Box::new(subs)),
            ..Default::default()
        }
    }

    #[test]
    fn detects_direct_failure_detail_property() {
        let s = schema_with_property("failure_detail_md");
        assert!(variant_defines_failure_detail(&s));
    }

    #[test]
    fn detects_failure_detail_via_all_of_recursion() {
        // Mirrors what schemars emits for `#[serde(flatten)] common: RowCommon`:
        // an `allOf` containing the variant's own props and a $ref-to-Common
        // child. We model the inner branch as the one that owns the field.
        let inner = schema_with_property("failure_detail_md");
        let outer = schema_with_all_of(vec![Schema::Object(inner)]);
        assert!(variant_defines_failure_detail(&outer));
    }

    #[test]
    fn returns_false_when_no_object_and_no_subschemas() {
        let s = SchemaObject::default();
        assert!(!variant_defines_failure_detail(&s));
    }

    #[test]
    fn returns_false_when_all_of_branches_lack_the_field() {
        let other = schema_with_property("status");
        let outer = schema_with_all_of(vec![Schema::Object(other)]);
        assert!(!variant_defines_failure_detail(&outer));
    }

    fn url_test_root() -> RootSchema {
        let mut root = RootSchema {
            schema: schema_with_property("all_check_runs_url"),
            ..Default::default()
        };
        root.definitions.insert(
            "GateRun".into(),
            Schema::Object(schema_with_property("url")),
        );
        root
    }

    #[test]
    fn tighten_url_fields_sets_format_and_pattern_on_top_level_url() {
        let mut root = url_test_root();
        tighten_url_fields(&mut root);

        let prop = root
            .schema
            .object
            .as_ref()
            .unwrap()
            .properties
            .get("all_check_runs_url")
            .unwrap();
        let Schema::Object(prop_obj) = prop else {
            panic!("expected object");
        };
        assert_eq!(prop_obj.format.as_deref(), Some("uri"));
        assert_eq!(
            prop_obj.string.as_ref().unwrap().pattern.as_deref(),
            Some("^https://")
        );
    }

    #[test]
    fn tighten_url_fields_sets_format_and_pattern_on_gate_run_url() {
        let mut root = url_test_root();
        tighten_url_fields(&mut root);

        let Schema::Object(gate_run) = root.definitions.get("GateRun").unwrap() else {
            panic!("expected object");
        };
        let prop = gate_run
            .object
            .as_ref()
            .unwrap()
            .properties
            .get("url")
            .unwrap();
        let Schema::Object(prop_obj) = prop else {
            panic!("expected object");
        };
        assert_eq!(prop_obj.format.as_deref(), Some("uri"));
        assert_eq!(
            prop_obj.string.as_ref().unwrap().pattern.as_deref(),
            Some("^https://")
        );
    }

    #[test]
    fn ignores_bool_branches_in_all_of() {
        // schemars uses `Schema::Bool(true)` as a stand-in for "any". The
        // recursion must skip those without panicking — a future variant
        // shape may legitimately mix Bool branches with Object ones.
        let outer = schema_with_all_of(vec![Schema::Bool(true)]);
        assert!(!variant_defines_failure_detail(&outer));
    }

    #[test]
    fn strip_removes_double_format_from_threshold_config_f64_fields() {
        // schemars annotates f64 with `format: "double"` by default.
        // Before strip: present at every numeric leaf. After strip:
        // absent. The serialized schema is the easiest place to check
        // because nested SchemaObject traversal is verbose.
        let mut schema = schemars::schema_for!(crate::threshold::ThresholdConfig);
        let before = serde_json::to_string(&schema).expect("serialize");
        assert!(
            before.contains("\"format\":\"double\""),
            "schemars baseline: f64 should carry `format: double` before strip"
        );
        strip_nonstandard_number_formats(&mut schema);
        let after = serde_json::to_string(&schema).expect("serialize");
        assert!(
            !after.contains("\"format\":\"double\""),
            "strip must remove `format: double` from numeric leaves; got: {after}"
        );
    }

    #[test]
    fn strip_preserves_standard_string_formats() {
        // Sanity: a uri-typed property in the wire schema (post URL
        // tightening) must NOT have its `format: "uri"` stripped. The
        // strip targets numeric formats only.
        let mut schema = schemars::schema_for!(crate::Scorecard);
        tighten_url_fields(&mut schema);
        strip_nonstandard_number_formats(&mut schema);
        let after = serde_json::to_string(&schema).expect("serialize");
        assert!(
            after.contains("\"format\":\"uri\""),
            "strip must preserve standard string formats (uri); got: {after}"
        );
    }
}
