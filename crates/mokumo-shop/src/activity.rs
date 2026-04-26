//! Activity-action literals for the Mokumo shop vertical.
//!
//! R13 invariant: the values returned by `as_str()` are byte-identical to the
//! un-prefixed verbs already stored in `activity_log.action` from pre-Stage-3
//! rows. The `entity_type` column (e.g. `"customer"`, `"garment"`) is the
//! disambiguator — the action literal itself never includes the vertical name.
//!
//! Only the subset of variants used by shop-vertical mutation adapters lives
//! here. Platform-emitted actions (`login_success`, `password_changed`,
//! `setup_completed`, `password_reset`, `recovery_codes_regenerated`) stay in
//! kikan, which owns the auth vertical.

/// Activity-log action enum for shop-vertical mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityAction {
    Created,
    Updated,
    SoftDeleted,
    Restored,
}

impl ActivityAction {
    /// Byte-identical to the pre-Stage-3 `Display` output in
    /// `kikan_types::activity::ActivityAction`. Changing any literal here is
    /// an activity-log continuity break (R13).
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::SoftDeleted => "soft_deleted",
            Self::Restored => "restored",
        }
    }
}

impl std::fmt::Display for ActivityAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_matches_pre_stage3_literals() {
        assert_eq!(ActivityAction::Created.as_str(), "created");
        assert_eq!(ActivityAction::Updated.as_str(), "updated");
        assert_eq!(ActivityAction::SoftDeleted.as_str(), "soft_deleted");
        assert_eq!(ActivityAction::Restored.as_str(), "restored");
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(ActivityAction::Created.to_string(), "created");
        assert_eq!(ActivityAction::SoftDeleted.to_string(), "soft_deleted");
    }

    #[test]
    fn never_emits_prefixed_literal() {
        for a in [
            ActivityAction::Created,
            ActivityAction::Updated,
            ActivityAction::SoftDeleted,
            ActivityAction::Restored,
        ] {
            assert!(
                !a.as_str().starts_with("customer_"),
                "R13 violation: prefixed literal '{}'",
                a.as_str()
            );
            assert!(
                !a.as_str().starts_with("garment_"),
                "R13 violation: prefixed literal '{}'",
                a.as_str()
            );
        }
    }
}
