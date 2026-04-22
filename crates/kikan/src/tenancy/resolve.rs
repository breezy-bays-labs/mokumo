use std::path::Path;
use std::str::FromStr;

/// Resolve the active profile from the on-disk `active_profile` file.
///
/// Generic over the vertical's profile discriminant: `K::from_str`
/// parses the file contents. On missing file, unreadable file, or parse
/// failure, returns `default` — callers pass
/// [`Graft::default_profile_kind`](crate::Graft::default_profile_kind).
pub fn resolve_active_profile<K>(data_dir: &Path, default: K) -> K
where
    K: FromStr,
{
    let profile_path = data_dir.join("active_profile");
    match std::fs::read_to_string(&profile_path) {
        Ok(contents) => contents.trim().parse().unwrap_or(default),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => default,
        Err(e) => {
            tracing::error!(
                path = %profile_path.display(),
                "Failed to read active_profile file: {e}; defaulting"
            );
            default
        }
    }
}
