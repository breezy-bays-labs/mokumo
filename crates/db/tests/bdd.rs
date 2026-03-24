use cucumber::World as _;

#[path = "bdd_world/mod.rs"]
mod bdd_world;

#[tokio::main]
async fn main() {
    bdd_world::DbWorld::cucumber()
        .filter_run_and_exit("tests/features", |feature, _, sc| {
            let dominated_by_wip =
                feature.tags.iter().any(|t| t == "wip") || sc.tags.iter().any(|t| t == "wip");
            !dominated_by_wip
        })
        .await;
}
