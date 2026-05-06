use cucumber::World as _;

#[path = "api_bdd_world/mod.rs"]
mod api_bdd_world;

/// Tags that exempt a scenario from the fail-on-skipped gate.
const SKIP_EXEMPT_TAGS: &[&str] = &["wip", "allow.skipped", "future"];

/// Tags that prevent the runner from executing a scenario at all.
///
/// `@wip` = work in progress; `@allow.skipped` = the cucumber-rs harness
/// genuinely cannot reproduce this failure mode (e.g., concurrent state
/// across requests, real read-only filesystem rollback). `@future`
/// scenarios do execute — they only tolerate a skip without forcing
/// one — matching how the tag is used elsewhere in the suite for
/// scenarios whose steps probe yet-unimplemented behaviour.
const FILTER_OUT_TAGS: &[&str] = &["wip", "allow.skipped"];

fn is_exempt(tags: &[String]) -> bool {
    tags.iter().any(|t| SKIP_EXEMPT_TAGS.contains(&t.as_str()))
}

fn is_filtered_out(tags: &[String]) -> bool {
    tags.iter().any(|t| FILTER_OUT_TAGS.contains(&t.as_str()))
}

#[tokio::main]
async fn main() {
    // Surface server-side `tracing::error!` lines into the cucumber-rs
    // captured-output buffer so a step that returns 500 because of a
    // platform error doesn't reduce to "An internal error occurred" in
    // the assertion message — the actual cause becomes visible in CI
    // logs alongside the panic. RUST_LOG can override at runtime.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("warn,mokumo_shop=debug,kikan=debug")
            }),
        )
        .with_test_writer()
        .try_init();

    // mokumo#655: open the JSONL sink before any scenario runs so the
    // `before(scenario)` hook below and the per-request capture layer
    // share one writer for the whole harness run.
    api_bdd_world::scenario_coverage::init_run("api_bdd");

    api_bdd_world::ApiWorld::cucumber()
        .fail_on_skipped_with(|feature, rule, scenario| {
            !is_exempt(&feature.tags)
                && rule.is_none_or(|r| !is_exempt(&r.tags))
                && !is_exempt(&scenario.tags)
        })
        .before(|feature, _rule, scenario, world| {
            // Stamp the World's recorder with the scenario about to run.
            // The capture middleware reads this slot on every request.
            // `gherkin::Feature::path` is `Option<PathBuf>` — fall back
            // to the title when the feature was loaded inline.
            let info = api_bdd_world::scenario_coverage::ScenarioInfo {
                feature_path: feature
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                feature_title: feature.name.clone(),
                scenario_name: scenario.name.clone(),
            };
            Box::pin(async move {
                world.scenario_recorder.set(info);
            })
        })
        .after(|_feature, _rule, _scenario, _ev, world| {
            Box::pin(async move {
                if let Some(w) = world {
                    w.scenario_recorder.clear();
                }
                api_bdd_world::scenario_coverage::flush_global();
            })
        })
        .filter_run("tests/api_features", |feature, _, sc| {
            !is_filtered_out(&feature.tags) && !is_filtered_out(&sc.tags)
        })
        .await;
}
