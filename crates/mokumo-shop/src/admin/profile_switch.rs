//! Wire-DTO wrapper for the admin UDS profile-switch surface.
//!
//! `kikan::control_plane::profiles::switch_profile_admin` is generic over
//! the graft's `ProfileKind` and returns a `(previous, current)` tuple.
//! This adapter fixes `K = SetupMode` and renders the
//! `ProfileSwitchAdminResponse` wire shape consumed by the admin client.

use kikan::{ControlPlaneError, PlatformState};
use kikan_types::SetupMode;
use kikan_types::admin::ProfileSwitchAdminResponse;

pub async fn switch_profile_admin(
    state: &PlatformState,
    target: SetupMode,
) -> Result<ProfileSwitchAdminResponse, ControlPlaneError> {
    let (previous, current) =
        kikan::control_plane::profiles::switch_profile_admin(state, target).await?;
    Ok(ProfileSwitchAdminResponse { previous, current })
}
