//! Producer pipeline tests — exercise the join logic with fixture data.

use super::*;
use crate::coverage::route_walker::RouteEntry;
use std::path::PathBuf;

fn route(crate_name: &str, method: &str, path: &str) -> RouteEntry {
    RouteEntry {
        method: method.into(),
        path: path.into(),
        rust_path: format!(
            "{crate_name}::{}",
            path.trim_start_matches('/').replace('/', "::")
        ),
        crate_name: crate_name.into(),
        source_file: PathBuf::from(format!("crates/{crate_name}/src/lib.rs")),
        source_line: 1,
    }
}

fn jsonl_row(method: &str, matched_path: &str, scenario: &str, status_class: &str) -> Row {
    Row {
        feature_path: "f.feature".into(),
        feature_title: "F".into(),
        scenario: scenario.into(),
        method: method.into(),
        matched_path: matched_path.into(),
        status: match status_class {
            "happy" => 200,
            "error_4xx" => 400,
            "error_5xx" => 500,
            _ => 0,
        },
        status_class: status_class.into(),
    }
}

#[test]
fn join_buckets_scenarios_by_status_class() {
    let routes = vec![route("mokumo-shop", "GET", "/api/customers")];
    let rows = vec![
        jsonl_row("GET", "/api/customers", "list returns all", "happy"),
        jsonl_row("GET", "/api/customers", "rejects unauth", "error_4xx"),
        jsonl_row("GET", "/api/customers", "list returns all", "happy"), // dedupe
    ];
    let (by_crate, orphans) = join_routes_with_observations(routes, &rows);
    assert!(orphans.is_empty());
    assert_eq!(by_crate.len(), 1);
    let h = &by_crate[0].handlers[0];
    assert_eq!(h.happy, vec!["list returns all".to_string()]);
    assert_eq!(h.error_4xx, vec!["rejects unauth".to_string()]);
    assert!(h.error_5xx.is_empty());
}

#[test]
fn join_emits_orphan_for_unwalked_path() {
    let routes = vec![route("mokumo-shop", "GET", "/api/customers")];
    let rows = vec![jsonl_row("GET", "/api/missing", "ghost", "happy")];
    let (by_crate, orphans) = join_routes_with_observations(routes, &rows);
    assert_eq!(by_crate.len(), 1);
    let h = &by_crate[0].handlers[0];
    assert!(h.happy.is_empty()); // walked route, but no matching observation
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].method, "GET");
    assert_eq!(orphans[0].matched_path, "/api/missing");
    assert_eq!(orphans[0].example_scenarios, vec!["ghost".to_string()]);
}

#[test]
fn join_dedupe_within_orphan_examples() {
    let routes = Vec::new();
    let rows = vec![
        jsonl_row("GET", "/api/missing", "scenario-a", "happy"),
        jsonl_row("GET", "/api/missing", "scenario-a", "error_4xx"),
        jsonl_row("GET", "/api/missing", "scenario-b", "happy"),
    ];
    let (_, orphans) = join_routes_with_observations(routes, &rows);
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].example_scenarios.len(), 2);
    assert!(orphans[0].example_scenarios.contains(&"scenario-a".into()));
    assert!(orphans[0].example_scenarios.contains(&"scenario-b".into()));
}

#[test]
fn join_orphan_examples_are_truncated() {
    let routes = Vec::new();
    let rows: Vec<Row> = (0..ORPHAN_EXAMPLE_LIMIT + 5)
        .map(|i| jsonl_row("GET", "/api/missing", &format!("scenario-{i}"), "happy"))
        .collect();
    let (_, orphans) = join_routes_with_observations(routes, &rows);
    assert_eq!(orphans[0].example_scenarios.len(), ORPHAN_EXAMPLE_LIMIT);
}

#[test]
fn join_method_match_is_case_insensitive_on_the_observed_side() {
    let routes = vec![route("x", "POST", "/api/x")];
    let rows = vec![jsonl_row("post", "/api/x", "s", "happy")];
    let (by_crate, orphans) = join_routes_with_observations(routes, &rows);
    assert!(orphans.is_empty());
    assert_eq!(by_crate[0].handlers[0].happy, vec!["s".to_string()]);
}

#[test]
fn join_groups_handlers_by_crate() {
    let routes = vec![
        route("kikan", "GET", "/api/health"),
        route("mokumo-shop", "GET", "/api/customers"),
    ];
    let rows = Vec::new();
    let (by_crate, _) = join_routes_with_observations(routes, &rows);
    assert_eq!(by_crate.len(), 2);
    assert_eq!(by_crate[0].crate_name, "kikan");
    assert_eq!(by_crate[1].crate_name, "mokumo-shop");
}

#[test]
fn join_collapses_duplicate_walker_emissions_for_same_route() {
    // The walker can emit the same (method, path, crate) twice when a
    // sub-router is mounted via `.merge(...)` and walked from both sites.
    let routes = vec![
        route("mokumo-shop", "GET", "/api/customers"),
        route("mokumo-shop", "GET", "/api/customers"),
    ];
    let rows = vec![jsonl_row("GET", "/api/customers", "s", "happy")];
    let (by_crate, _) = join_routes_with_observations(routes, &rows);
    assert_eq!(by_crate[0].handlers.len(), 1);
}

// ---------------------------------------------------------------------
// End-to-end FS tests covering run / discover_crates /
// scan_subdir_for_packages / read_package_name / gather_walker_inputs.
// Mock workspace = one tempdir crate with a single literal route + an
// empty crap4rs.toml so excluded_crates stays bounded.
// ---------------------------------------------------------------------

fn write_mock_workspace(root: &std::path::Path, crate_name: &str, route_lit: &str) {
    let crate_dir = root.join("crates").join(crate_name);
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        format!("[package]\nname = \"{crate_name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    std::fs::write(
        crate_dir.join("src/lib.rs"),
        format!(
            "use axum::{{Router, routing::get}};\npub fn router() -> Router {{ Router::new().route(\"{route_lit}\", get(handler)) }}\nfn handler() {{}}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("crap4rs.toml"),
        "preset = \"strict\"\nexclude = []\n",
    )
    .unwrap();
}

#[test]
fn run_walks_mock_workspace_and_emits_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    write_mock_workspace(tmp.path(), "demo", "/api/health");
    let input = ProducerInput {
        workspace_root: tmp.path().to_path_buf(),
        jsonl_dir: tmp.path().join("bdd-coverage"),
        now_override: Some("2026-05-06T00:00:00Z".into()),
    };
    let output = run(&input).unwrap();
    assert_eq!(output.artifact.version, ARTIFACT_VERSION);
    assert_eq!(output.artifact.generated_at, "2026-05-06T00:00:00Z");
    assert_eq!(output.artifact.diagnostics.rows_consumed, 0);
    assert_eq!(output.artifact.by_crate.len(), 1);
    assert_eq!(output.artifact.by_crate[0].crate_name, "demo");
    assert_eq!(output.artifact.by_crate[0].handlers[0].path, "/api/health");
    assert_eq!(output.exit_code, 0);
}

#[test]
fn run_with_observations_credits_handler_and_drops_orphans() {
    let tmp = tempfile::tempdir().unwrap();
    write_mock_workspace(tmp.path(), "demo", "/api/health");
    let jsonl_dir = tmp.path().join("bdd-coverage");
    std::fs::create_dir_all(&jsonl_dir).unwrap();
    std::fs::write(
        jsonl_dir.join("api_bdd-1.jsonl"),
        "{\"feature_path\":\"f\",\"feature_title\":\"F\",\"scenario\":\"healthcheck\",\"method\":\"GET\",\"matched_path\":\"/api/health\",\"status\":200,\"status_class\":\"happy\"}\n{\"feature_path\":\"f\",\"feature_title\":\"F\",\"scenario\":\"missing\",\"method\":\"GET\",\"matched_path\":\"/api/ghost\",\"status\":404,\"status_class\":\"error_4xx\"}\n",
    )
    .unwrap();
    let input = ProducerInput {
        workspace_root: tmp.path().to_path_buf(),
        jsonl_dir,
        now_override: Some("now".into()),
    };
    let output = run(&input).unwrap();
    let h = &output.artifact.by_crate[0].handlers[0];
    assert_eq!(h.happy, vec!["healthcheck".to_string()]);
    assert_eq!(output.artifact.diagnostics.orphan_observations.len(), 1);
    assert_eq!(output.artifact.diagnostics.rows_consumed, 2);
    assert_eq!(output.exit_code, 2);
}

#[test]
fn run_errors_when_workspace_has_no_crates() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("crap4rs.toml"),
        "preset = \"strict\"\nexclude = []\n",
    )
    .unwrap();
    let input = ProducerInput {
        workspace_root: tmp.path().to_path_buf(),
        jsonl_dir: tmp.path().join("nope"),
        now_override: None,
    };
    let err = run(&input).unwrap_err();
    assert!(err.to_string().contains("no crates discovered"));
}

#[test]
fn run_skips_subdirs_without_cargo_toml() {
    let tmp = tempfile::tempdir().unwrap();
    write_mock_workspace(tmp.path(), "demo", "/api/x");
    std::fs::create_dir_all(tmp.path().join("crates/scratchpad")).unwrap();
    std::fs::write(tmp.path().join("crates/scratchpad/notes.md"), "junk").unwrap();
    let input = ProducerInput {
        workspace_root: tmp.path().to_path_buf(),
        jsonl_dir: tmp.path().join("nope"),
        now_override: None,
    };
    let output = run(&input).unwrap();
    assert_eq!(
        output.artifact.by_crate.len(),
        1,
        "scratchpad without Cargo.toml should be skipped"
    );
}

#[test]
fn run_skips_cargo_toml_without_package_table() {
    let tmp = tempfile::tempdir().unwrap();
    write_mock_workspace(tmp.path(), "demo", "/api/x");
    std::fs::create_dir_all(tmp.path().join("crates/workspace-only")).unwrap();
    std::fs::write(
        tmp.path().join("crates/workspace-only/Cargo.toml"),
        "[workspace]\nmembers = []\n",
    )
    .unwrap();
    let input = ProducerInput {
        workspace_root: tmp.path().to_path_buf(),
        jsonl_dir: tmp.path().join("nope"),
        now_override: None,
    };
    let output = run(&input).unwrap();
    assert_eq!(output.artifact.by_crate.len(), 1);
    assert_eq!(output.artifact.by_crate[0].crate_name, "demo");
}

#[test]
fn run_walks_apps_subdir_too() {
    let tmp = tempfile::tempdir().unwrap();
    let crate_dir = tmp.path().join("apps/demo-server");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"demo-server\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        crate_dir.join("src/lib.rs"),
        "use axum::{Router, routing::get};\npub fn r() -> Router { Router::new().route(\"/api/x\", get(h)) }\nfn h() {}\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("crap4rs.toml"),
        "preset = \"strict\"\nexclude = []\n",
    )
    .unwrap();
    let input = ProducerInput {
        workspace_root: tmp.path().to_path_buf(),
        jsonl_dir: tmp.path().join("nope"),
        now_override: None,
    };
    let output = run(&input).unwrap();
    assert_eq!(output.artifact.by_crate[0].crate_name, "demo-server");
}

#[test]
fn now_iso8601_returns_iso_shape() {
    let s = now_iso8601();
    assert_eq!(s.len(), 20);
    assert!(s.ends_with('Z'));
    assert_eq!(s.as_bytes()[10], b'T');
}

#[test]
fn epoch_to_ymdhms_handles_known_epoch_seconds() {
    // 1_700_000_000 = 2023-11-14T22:13:20Z (well-known).
    let (y, m, d, hh, mm, ss) = epoch_to_ymdhms(1_700_000_000);
    assert_eq!((y, m, d), (2023, 11, 14));
    assert_eq!((hh, mm, ss), (22, 13, 20));
}
