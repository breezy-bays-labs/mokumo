#[path = "support/mod.rs"]
mod support;

use kikan::migrations::dag;
use kikan::{DagError, Migration, MigrationTarget};
use std::sync::Arc;
use support::make_migration;

fn arc_migrations(migrations: Vec<Box<dyn Migration>>) -> Vec<Arc<dyn Migration>> {
    migrations.into_iter().map(Arc::from).collect()
}

fn make_meta(name: &'static str, deps: Vec<&'static str>) -> Box<dyn Migration> {
    make_migration(name, deps, MigrationTarget::Meta)
}

fn make_per_profile(name: &'static str, deps: Vec<&'static str>) -> Box<dyn Migration> {
    make_migration(name, deps, MigrationTarget::PerProfile)
}

// --- Happy path: topological ordering ---

#[test]
fn empty_set_resolves_to_empty() {
    let result = dag::resolve(&[]);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn single_migration() {
    let migrations = arc_migrations(vec![make_per_profile("A", vec![])]);
    let result = dag::resolve(&migrations).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name(), "A");
}

#[test]
fn linear_two_chain() {
    let migrations = arc_migrations(vec![
        make_per_profile("A", vec![]),
        make_per_profile("B", vec!["A"]),
    ]);
    let result = dag::resolve(&migrations).unwrap();
    let names: Vec<&str> = result.iter().map(|m| m.name()).collect();
    assert_eq!(names, vec!["A", "B"]);
}

#[test]
fn linear_three_chain() {
    let migrations = arc_migrations(vec![
        make_per_profile("C", vec!["B"]),
        make_per_profile("A", vec![]),
        make_per_profile("B", vec!["A"]),
    ]);
    let result = dag::resolve(&migrations).unwrap();
    let names: Vec<&str> = result.iter().map(|m| m.name()).collect();
    assert_eq!(names, vec!["A", "B", "C"]);
}

#[test]
fn two_independent_chains() {
    let migrations = arc_migrations(vec![
        make_per_profile("B", vec!["A"]),
        make_per_profile("A", vec![]),
        make_per_profile("D", vec!["C"]),
        make_per_profile("C", vec![]),
    ]);
    let result = dag::resolve(&migrations).unwrap();
    let names: Vec<&str> = result.iter().map(|m| m.name()).collect();
    let pos_a = names.iter().position(|n| *n == "A").unwrap();
    let pos_b = names.iter().position(|n| *n == "B").unwrap();
    let pos_c = names.iter().position(|n| *n == "C").unwrap();
    let pos_d = names.iter().position(|n| *n == "D").unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_c < pos_d);
}

#[test]
fn diamond_dependency() {
    let migrations = arc_migrations(vec![
        make_per_profile("D", vec!["B", "C"]),
        make_per_profile("C", vec!["A"]),
        make_per_profile("B", vec!["A"]),
        make_per_profile("A", vec![]),
    ]);
    let result = dag::resolve(&migrations).unwrap();
    let names: Vec<&str> = result.iter().map(|m| m.name()).collect();

    assert_eq!(names.len(), 4, "each migration appears exactly once");
    let pos_a = names.iter().position(|n| *n == "A").unwrap();
    let pos_b = names.iter().position(|n| *n == "B").unwrap();
    let pos_c = names.iter().position(|n| *n == "C").unwrap();
    let pos_d = names.iter().position(|n| *n == "D").unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_a < pos_c);
    assert!(pos_b < pos_d);
    assert!(pos_c < pos_d);
}

// --- Dependency violations ---

#[test]
fn self_loop_detected() {
    let migrations = arc_migrations(vec![make_per_profile("A", vec!["A"])]);
    let result = dag::resolve(&migrations);
    assert!(matches!(result, Err(DagError::Cycle { .. })));
}

#[test]
fn two_node_cycle_detected() {
    let migrations = arc_migrations(vec![
        make_per_profile("A", vec!["B"]),
        make_per_profile("B", vec!["A"]),
    ]);
    let result = dag::resolve(&migrations);
    assert!(matches!(result, Err(DagError::Cycle { .. })));
}

#[test]
fn three_node_cycle_in_five_node_graph() {
    let migrations = arc_migrations(vec![
        make_per_profile("A", vec![]),
        make_per_profile("B", vec!["A"]),
        make_per_profile("C", vec!["B"]),
        make_per_profile("D", vec!["C"]),
        make_per_profile("E", vec!["D"]),
    ]);
    let result = dag::resolve(&migrations);
    assert!(result.is_ok(), "linear graph should not have a cycle");

    let cycle_migrations = arc_migrations(vec![
        make_per_profile("A", vec![]),
        make_per_profile("B", vec!["A", "E"]),
        make_per_profile("C", vec!["B"]),
        make_per_profile("D", vec!["C"]),
        make_per_profile("E", vec!["D"]),
    ]);
    let result = dag::resolve(&cycle_migrations);
    assert!(
        matches!(result, Err(DagError::Cycle { .. })),
        "cycle among B->C->D->E->B should be detected"
    );
}

#[test]
fn dangling_dependency_reference() {
    let migrations = arc_migrations(vec![make_per_profile("A", vec!["nonexistent"])]);
    let result = dag::resolve(&migrations);
    assert!(matches!(result, Err(DagError::DanglingRef { .. })));
}

#[test]
fn duplicate_migration_name_within_graft() {
    let migrations = arc_migrations(vec![
        make_per_profile("A", vec![]),
        make_per_profile("A", vec![]),
    ]);
    let result = dag::resolve(&migrations);
    assert!(matches!(result, Err(DagError::DuplicateMigration { .. })));
}

// --- Cross-target ordering ---

#[test]
fn meta_ordered_before_per_profile() {
    let migrations = arc_migrations(vec![make_per_profile("P", vec![]), make_meta("M", vec![])]);
    let result = dag::resolve(&migrations).unwrap();
    let names: Vec<&str> = result.iter().map(|m| m.name()).collect();
    let pos_m = names.iter().position(|n| *n == "M").unwrap();
    let pos_p = names.iter().position(|n| *n == "P").unwrap();
    assert!(
        pos_m < pos_p,
        "Meta migration should be ordered before PerProfile"
    );
}

#[test]
fn per_profile_may_depend_on_meta() {
    let migrations = arc_migrations(vec![
        make_per_profile("P", vec!["M"]),
        make_meta("M", vec![]),
    ]);
    let result = dag::resolve(&migrations);
    assert!(result.is_ok(), "PerProfile depending on Meta is valid");
    let ordered = result.unwrap();
    let names: Vec<&str> = ordered.iter().map(|m| m.name()).collect();
    assert_eq!(names, vec!["M", "P"]);
}

#[test]
fn meta_depending_on_per_profile_rejected() {
    let migrations = arc_migrations(vec![
        make_meta("M", vec!["P"]),
        make_per_profile("P", vec![]),
    ]);
    let result = dag::resolve(&migrations);
    assert!(
        matches!(result, Err(DagError::CrossTargetViolation { .. })),
        "Meta depending on PerProfile should be rejected"
    );
}

// --- Determinism proptest ---

mod proptest_determinism {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn resolve_is_deterministic(_ in 0..100u32) {
            let migrations = arc_migrations(vec![
                make_per_profile("D", vec!["B", "C"]),
                make_per_profile("C", vec!["A"]),
                make_per_profile("B", vec!["A"]),
                make_per_profile("A", vec![]),
                make_meta("M1", vec![]),
                make_meta("M2", vec!["M1"]),
            ]);

            let result1 = dag::resolve(&migrations).unwrap();
            let result2 = dag::resolve(&migrations).unwrap();

            let names1: Vec<&str> = result1.iter().map(|m| m.name()).collect();
            let names2: Vec<&str> = result2.iter().map(|m| m.name()).collect();
            prop_assert_eq!(names1, names2);
        }
    }
}
