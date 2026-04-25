Feature: Legacy completed install migrates into meta.profiles

  When the engine boots and finds a `production/` data directory with
  admin user(s) and a usable `shop_settings.shop_name`, it derives a
  kebab-case slug and records the install in `meta.profiles` plus a
  `meta.activity_log` audit entry — atomically, in a single meta-DB
  transaction.

  PR A scope: meta-only. The on-disk `production/` folder and the
  `<data_dir>/active_profile` pointer are intentionally NOT modified;
  the binary's `prepare_database` continues to address the legacy install
  as `production` until PR B refactors those call sites to consult
  `meta.profiles`. Idempotency on the next boot is provided by
  `detect_boot_state` returning `PostUpgradeOrSetup` once any row exists.

  See `crates/kikan/src/meta/upgrade.rs` and the M00 shape doc §Seam 2.

  Scenario: legacy completed install records derived slug in meta.profiles
    Given a legacy production database with an admin user and shop_name "Acme Printing"
    When the engine boots
    Then the engine boots successfully
    And meta.profiles has a row with slug "acme-printing" and display_name "Acme Printing"
    And meta.activity_log has a legacy_upgrade_migrated entry for "acme-printing"
    And the on-disk production folder is unchanged
