use cucumber::World as _;

#[path = "api_bdd_world/mod.rs"]
mod api_bdd_world;

/// Tags that exempt a scenario from the fail-on-skipped gate.
const SKIP_EXEMPT_TAGS: &[&str] = &["wip", "allow.skipped", "future"];

fn is_exempt(tags: &[String]) -> bool {
    tags.iter().any(|t| SKIP_EXEMPT_TAGS.contains(&t.as_str()))
}

#[tokio::main]
async fn main() {
    api_bdd_world::ApiWorld::cucumber()
        .fail_on_skipped_with(|feature, rule, scenario| {
            !is_exempt(&feature.tags)
                && rule.is_none_or(|r| !is_exempt(&r.tags))
                && !is_exempt(&scenario.tags)
        })
        .filter_run("tests/api_features", |feature, _, sc| {
            let dominated_by_wip =
                feature.tags.iter().any(|t| t == "wip") || sc.tags.iter().any(|t| t == "wip");
            !dominated_by_wip
        })
        .await;
}
