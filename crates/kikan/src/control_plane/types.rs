//! Kikan-owned supporting types for the `Graft` hooks that carry
//! vertical-specific recovery and bootstrap vocabulary.
//!
//! These types are the kikan side of the capability/vocabulary split
//! recorded in `adr-kikan-engine-vocabulary` § "Amendment 2026-04-22 (b)".
//! The vertical implements `Graft::recovery_dir`, `Graft::setup_token_source`,
//! and `Graft::valid_reset_pin_ids`; kikan reads the returned values as
//! opaque data and never matches on their contents.
//!
//! # Types
//!
//! - [`SetupTokenSource`] — enum telling the engine where to obtain the
//!   first-admin bootstrap token (or that the vertical does not use one).
//! - [`PinId`] — validated opaque identifier for a reset PIN, following
//!   the [`ProfileDirName`](crate::tenancy::ProfileDirName) precedent:
//!   `Arc<str>` inside, validated at construction, `new_trusted` for
//!   compile-time-known inputs.

use std::path::PathBuf;
use std::sync::Arc;

/// Source of the first-admin bootstrap token.
///
/// Returned by [`Graft::setup_token_source`](crate::Graft::setup_token_source).
/// The engine resolves the variant once at boot and stashes the effective
/// token on [`ControlPlaneState`](crate::ControlPlaneState) for the
/// `setup_admin` pure-fn to compare against the caller-supplied token.
///
/// The engine never inspects token contents — it only reads the file (for
/// `File`), clones the `Arc<str>` (for `Inline`), or records `None` (for
/// `Disabled`).
#[derive(Debug, Clone)]
pub enum SetupTokenSource {
    /// The vertical does not use a setup-wizard token. The engine records
    /// `setup_token = None`; the setup handler will always reject callers
    /// with `PermissionDenied` (or the vertical routes around the handler
    /// entirely).
    Disabled,
    /// Read the token synchronously from the given filesystem path at boot.
    ///
    /// Resolution rules (enforced in `Engine::boot`):
    /// - I/O error surfaces as [`EngineError::Boot`](crate::EngineError::Boot);
    ///   the engine refuses to start rather than run with an indeterminate
    ///   token (fail-fast per ADR amendment 2026-04-22 (a)).
    /// - The file content is trimmed of leading/trailing whitespace.
    /// - An empty file (or file whose content trims to empty) is normalized
    ///   to the [`Disabled`](Self::Disabled) posture — a zero-length token
    ///   would otherwise match an empty request body and silently permit
    ///   unauthenticated bootstrap.
    File(PathBuf),
    /// The vertical hands the engine an already-resolved token value
    /// directly. Cloning is a refcount bump.
    ///
    /// An empty `Arc<str>` is normalized to the [`Disabled`](Self::Disabled)
    /// posture at boot (same reasoning as `File` with an empty file).
    Inline(Arc<str>),
}

/// Opaque identifier for a valid reset PIN (vertical-declared).
///
/// Kikan never matches on `PinId` contents — it only stores it, compares
/// equal, hashes it, or displays it. The vertical supplies a `'static`
/// slice via [`Graft::valid_reset_pin_ids`](crate::Graft::valid_reset_pin_ids);
/// kikan iterates it without copying.
///
/// Construction rejects:
/// - empty strings
/// - strings that are whitespace-only (empty after trim)
/// - strings containing a NUL byte
///
/// The rules are proportional to the domain: `PinId` is an opaque
/// identifier, not a filesystem path, so path-separator and leading-dot
/// checks are not applicable. The rejections here close the programmer-error
/// and embedded-NUL classes that a raw `String` would invite.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct PinId(Arc<str>);

/// Rejection reason returned by [`PinId::new`] and [`PinId::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinIdError {
    Empty,
    WhitespaceOnly,
    InteriorNul,
}

impl std::fmt::Display for PinIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Empty => "PIN id is empty",
            Self::WhitespaceOnly => "PIN id is whitespace only",
            Self::InteriorNul => "PIN id contains a NUL byte",
        };
        f.write_str(s)
    }
}

impl std::error::Error for PinIdError {}

fn validate(s: &str) -> Result<(), PinIdError> {
    if s.is_empty() {
        return Err(PinIdError::Empty);
    }
    if s.trim().is_empty() {
        return Err(PinIdError::WhitespaceOnly);
    }
    if s.contains('\0') {
        return Err(PinIdError::InteriorNul);
    }
    Ok(())
}

impl PinId {
    /// Construct a validated `PinId`. Returns `Err` if the input is empty,
    /// whitespace-only, or contains a NUL byte.
    pub fn new(id: impl Into<Arc<str>>) -> Result<Self, PinIdError> {
        let arc: Arc<str> = id.into();
        validate(&arc)?;
        Ok(Self(arc))
    }

    /// Parse a `&str` into a `PinId`, validating.
    pub fn parse(s: &str) -> Result<Self, PinIdError> {
        validate(s)?;
        Ok(Self(Arc::from(s)))
    }

    /// Construct without returning a `Result` — for trusted callers where
    /// a validation failure would signal a programming error.
    ///
    /// # Panics
    ///
    /// Panics if `id` fails the [`validate`] checks. Use when the input is
    /// already known to satisfy the invariants (e.g., a module-level
    /// constant in the vertical).
    pub fn new_trusted(id: impl Into<Arc<str>>) -> Self {
        let arc: Arc<str> = id.into();
        if let Err(e) = validate(&arc) {
            panic!("PinId::new_trusted received invalid input {arc:?}: {e}");
        }
        Self(arc)
    }

    /// Borrow the inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PinId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&*self.0, f)
    }
}

impl AsRef<str> for PinId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for PinId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for PinId {
    type Err = PinIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// `From<&'static str>` stays infallible for module-level constants
/// declared by the vertical. Panics on an invalid literal at the call
/// site — a programmer error, not a user-facing condition.
impl From<&'static str> for PinId {
    fn from(s: &'static str) -> Self {
        Self::new_trusted(s)
    }
}

impl<'de> serde::Deserialize<'de> for PinId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s: String = serde::Deserialize::deserialize(d)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn pin_id_accepts_normal_identifiers() {
        assert_eq!(PinId::parse("PIN-A").unwrap().as_str(), "PIN-A");
        assert_eq!(PinId::parse("m0_reset").unwrap().as_str(), "m0_reset");
        assert_eq!(PinId::parse("abc123").unwrap().as_str(), "abc123");
    }

    #[test]
    fn pin_id_accepts_leading_dot() {
        // PinId is not a filesystem path, so leading-dot is fine.
        assert_eq!(PinId::parse(".hidden").unwrap().as_str(), ".hidden");
    }

    #[test]
    fn pin_id_accepts_path_separators() {
        // PinId is opaque to kikan; not a path. `/` and `\` are allowed.
        assert_eq!(PinId::parse("foo/bar").unwrap().as_str(), "foo/bar");
    }

    #[test]
    fn pin_id_rejects_empty() {
        assert_eq!(PinId::parse(""), Err(PinIdError::Empty));
    }

    #[test]
    fn pin_id_rejects_whitespace_only() {
        assert_eq!(PinId::parse("   "), Err(PinIdError::WhitespaceOnly));
        assert_eq!(PinId::parse("\t"), Err(PinIdError::WhitespaceOnly));
        assert_eq!(PinId::parse("\n"), Err(PinIdError::WhitespaceOnly));
        assert_eq!(PinId::parse(" \t\n"), Err(PinIdError::WhitespaceOnly));
    }

    #[test]
    fn pin_id_rejects_interior_nul() {
        assert_eq!(PinId::parse("abc\0def"), Err(PinIdError::InteriorNul));
        assert_eq!(PinId::parse("\0"), Err(PinIdError::InteriorNul));
    }

    #[test]
    fn pin_id_from_str_matches_parse() {
        use std::str::FromStr;
        assert_eq!(PinId::from_str("demo").unwrap().as_str(), "demo");
        assert_eq!(PinId::from_str(""), Err(PinIdError::Empty));
    }

    #[test]
    fn pin_id_deserialize_accepts_valid() {
        let decoded: PinId = serde_json::from_str("\"PIN-A\"").unwrap();
        assert_eq!(decoded.as_str(), "PIN-A");
    }

    #[test]
    fn pin_id_deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<PinId>("\"\"").is_err());
        assert!(serde_json::from_str::<PinId>("\"   \"").is_err());
    }

    #[test]
    fn pin_id_serialize_roundtrips_as_transparent_string() {
        let id = PinId::parse("PIN-A").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"PIN-A\"");
        let decoded: PinId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn pin_id_new_trusted_accepts_valid() {
        let id = PinId::new_trusted("PIN-A");
        assert_eq!(id.as_str(), "PIN-A");
    }

    #[test]
    #[should_panic(expected = "PinId::new_trusted received invalid input")]
    fn pin_id_new_trusted_panics_on_invalid() {
        let _ = PinId::new_trusted("");
    }

    #[test]
    fn pin_id_from_static_str_accepts_valid() {
        let id: PinId = "PIN-A".into();
        assert_eq!(id.as_str(), "PIN-A");
    }

    #[test]
    #[should_panic(expected = "PinId::new_trusted received invalid input")]
    fn pin_id_from_static_str_panics_on_invalid() {
        let _: PinId = "".into();
    }

    #[test]
    fn pin_id_display_outputs_inner_string() {
        let id = PinId::parse("PIN-A").unwrap();
        assert_eq!(format!("{id}"), "PIN-A");
    }

    #[test]
    fn pin_id_hashmap_lookup_via_borrow_str_works() {
        let mut map: HashMap<PinId, i32> = HashMap::new();
        map.insert(PinId::parse("PIN-A").unwrap(), 42);
        assert_eq!(map.get("PIN-A"), Some(&42));
    }

    #[test]
    fn pin_id_clone_is_refcount_bump_not_alloc() {
        let a = PinId::parse("PIN-A").unwrap();
        let b = a.clone();
        assert!(std::ptr::eq(a.as_str().as_ptr(), b.as_str().as_ptr()));
    }

    #[test]
    fn setup_token_source_constructs_all_variants() {
        let _disabled = SetupTokenSource::Disabled;
        let _file = SetupTokenSource::File(PathBuf::from("/tmp/setup-token"));
        let _inline: SetupTokenSource = SetupTokenSource::Inline(Arc::from("tok"));
    }

    #[test]
    fn pin_id_error_display_mentions_kind() {
        assert_eq!(format!("{}", PinIdError::Empty), "PIN id is empty");
        assert_eq!(
            format!("{}", PinIdError::WhitespaceOnly),
            "PIN id is whitespace only"
        );
        assert_eq!(
            format!("{}", PinIdError::InteriorNul),
            "PIN id contains a NUL byte"
        );
    }
}
