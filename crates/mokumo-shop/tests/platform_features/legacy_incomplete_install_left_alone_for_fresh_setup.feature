Feature: Legacy incomplete install leaves the operator in a fresh-setup path

  An operator who started the setup wizard but never finished it leaves
  behind a `production/` folder with either no vertical DB at all or a
  vertical DB with no admin user. The engine treats this as
  `BootState::LegacyAbandoned` and proceeds with the normal boot —
  it does NOT run the legacy upgrade and does NOT INSERT a row into
  `meta.profiles`. The operator is dropped back at the setup wizard.

  See `crates/kikan/src/meta/boot_state.rs` (`AbandonReason::NoAdminUser`).

  Scenario: legacy abandoned install leaves meta.profiles empty
    Given a legacy production folder with a vertical DB that has no admin user
    When the engine boots
    Then the engine boots successfully
    And meta.profiles has no rows
