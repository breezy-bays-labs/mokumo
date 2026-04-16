use cucumber::World as _;

#[path = "bdd_world/mod.rs"]
mod bdd_world;

const SKIP_EXEMPT_TAGS: &[&str] = &["wip", "allow.skipped", "future", "integration"];

fn is_exempt(tags: &[String]) -> bool {
    tags.iter().any(|t| SKIP_EXEMPT_TAGS.contains(&t.as_str()))
}

#[tokio::main]
async fn main() {
    bdd_world::SchedulerWorld::cucumber()
        .fail_on_skipped_with(|feature, rule, scenario| {
            !is_exempt(&feature.tags)
                && rule.is_none_or(|r| !is_exempt(&r.tags))
                && !is_exempt(&scenario.tags)
        })
        .filter_run_and_exit("tests/features", |feature, _, sc| {
            let dominated_by_wip =
                feature.tags.iter().any(|t| t == "wip") || sc.tags.iter().any(|t| t == "wip");
            let is_integration = feature.tags.iter().any(|t| t == "integration")
                || sc.tags.iter().any(|t| t == "integration");
            !dominated_by_wip && !is_integration
        })
        .await;
}
