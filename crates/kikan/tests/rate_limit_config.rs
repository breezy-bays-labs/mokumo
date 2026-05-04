use std::time::Duration;

use kikan::boot::RateLimitConfig;

/// Default RateLimitConfig matches the production values wired at engine boot.
#[test]
fn default_rate_limit_config_matches_production_values() {
    let config = RateLimitConfig::default();

    // login: 10 per 15 min
    assert_eq!(config.login.max_attempts, 10);
    assert_eq!(config.login.window, Duration::from_mins(15));

    // recovery: 5 per 15 min
    assert_eq!(config.recovery.max_attempts, 5);
    assert_eq!(config.recovery.window, Duration::from_mins(15));

    // regen: 3 per 1 hour
    assert_eq!(config.regen.max_attempts, 3);
    assert_eq!(config.regen.window, Duration::from_hours(1));

    // profile_switch: 3 per 15 min
    assert_eq!(config.profile_switch.max_attempts, 3);
    assert_eq!(config.profile_switch.window, Duration::from_mins(15));

    // restore: 5 per 1 hour
    assert_eq!(config.restore.max_attempts, 5);
    assert_eq!(config.restore.window, Duration::from_hours(1));
}

/// RateWindow can be constructed with custom values.
#[test]
fn rate_window_custom_values() {
    let window = kikan::boot::RateWindow {
        max_attempts: 100,
        window: Duration::from_secs(1),
    };
    assert_eq!(window.max_attempts, 100);
    assert_eq!(window.window, Duration::from_secs(1));
}

/// BootConfig includes rate_limit_config with sensible defaults.
#[test]
fn boot_config_includes_rate_limit_config() {
    use kikan::BootConfig;

    let config = BootConfig::new(std::path::PathBuf::from("/tmp/test"));
    // Should have default rate limit config
    assert_eq!(config.rate_limit_config.login.max_attempts, 10);
}
