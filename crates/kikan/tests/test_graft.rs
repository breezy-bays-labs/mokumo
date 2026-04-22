//! `TestGraft`: validates that `Graft::ProfileKind` is genuinely opaque
//! to kikan.
//!
//! `StubGraft` sets `type ProfileKind = SetupMode`, which matches
//! Mokumo's choice — useful for writing tests that exercise the real
//! boot path, but it does not prove kikan could carry a different
//! vertical's `ProfileKind`. `TestGraft` uses a fixture enum
//! (`TestKind::{Alpha, Beta}`) with a custom filename (`"test.db"`) so
//! the associated-type bounds and vocabulary hooks are exercised
//! independent of Mokumo's vocabulary.

use kikan::{EngineContext, EngineError, Graft, GraftId, Migration};

/// Two-variant test profile kind. Intentionally not Mokumo's `SetupMode`
/// — the whole point of this fixture is proving kikan doesn't require it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    Alpha,
    Beta,
}

impl std::fmt::Display for TestKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestKind::Alpha => write!(f, "alpha"),
            TestKind::Beta => write!(f, "beta"),
        }
    }
}

impl std::str::FromStr for TestKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "alpha" => Ok(TestKind::Alpha),
            "beta" => Ok(TestKind::Beta),
            other => Err(format!("unknown test kind: {other}")),
        }
    }
}

static TEST_PROFILE_KINDS: &[TestKind] = &[TestKind::Alpha, TestKind::Beta];

#[derive(Clone)]
pub struct TestAppState {
    pub control_plane: kikan::ControlPlaneState,
}

pub struct TestGraft;

impl Graft for TestGraft {
    type AppState = TestAppState;
    type DomainState = ();
    type ProfileKind = TestKind;

    fn id() -> GraftId {
        GraftId::new("test")
    }

    fn db_filename(&self) -> &'static str {
        "test.db"
    }

    fn all_profile_kinds(&self) -> &'static [TestKind] {
        TEST_PROFILE_KINDS
    }

    fn default_profile_kind(&self) -> TestKind {
        TestKind::Alpha
    }

    fn profile_dir_name(&self, kind: &TestKind) -> &'static str {
        match kind {
            TestKind::Alpha => "alpha",
            TestKind::Beta => "beta",
        }
    }

    fn requires_setup_wizard(&self, _kind: &TestKind) -> bool {
        false
    }

    fn auth_profile_kind(&self) -> TestKind {
        TestKind::Alpha
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        Vec::new()
    }

    async fn build_domain_state(
        &self,
        _ctx: &EngineContext,
    ) -> Result<Self::DomainState, EngineError> {
        Ok(())
    }

    fn compose_state(
        control_plane: kikan::ControlPlaneState,
        _domain: Self::DomainState,
    ) -> Self::AppState {
        TestAppState { control_plane }
    }

    fn platform_state(state: &Self::AppState) -> &kikan::PlatformState {
        &state.control_plane.platform
    }

    fn control_plane_state(state: &Self::AppState) -> &kikan::ControlPlaneState {
        &state.control_plane
    }

    fn data_plane_routes(_state: &Self::AppState) -> axum::Router<Self::AppState> {
        axum::Router::new()
    }
}

#[test]
fn test_graft_db_filename_is_honored() {
    let graft = TestGraft;
    assert_eq!(graft.db_filename(), "test.db");
}

#[test]
fn test_graft_exposes_all_profile_kinds() {
    let graft = TestGraft;
    assert_eq!(
        graft.all_profile_kinds(),
        &[TestKind::Alpha, TestKind::Beta]
    );
}

#[test]
fn test_graft_default_profile_kind_is_alpha() {
    let graft = TestGraft;
    assert_eq!(graft.default_profile_kind(), TestKind::Alpha);
}

#[test]
fn test_graft_profile_dir_name_per_kind() {
    let graft = TestGraft;
    assert_eq!(graft.profile_dir_name(&TestKind::Alpha), "alpha");
    assert_eq!(graft.profile_dir_name(&TestKind::Beta), "beta");
}

#[test]
fn test_graft_no_kind_requires_setup_wizard() {
    let graft = TestGraft;
    assert!(!graft.requires_setup_wizard(&TestKind::Alpha));
    assert!(!graft.requires_setup_wizard(&TestKind::Beta));
}

#[test]
fn test_kind_roundtrips_via_fromstr_and_display() {
    use std::str::FromStr;
    for kind in [TestKind::Alpha, TestKind::Beta] {
        let s = kind.to_string();
        let parsed = TestKind::from_str(&s).expect("round-trip parse");
        assert_eq!(parsed, kind);
    }
}

#[test]
fn test_kind_from_str_rejects_unknown() {
    use std::str::FromStr;
    assert!(TestKind::from_str("gamma").is_err());
}
