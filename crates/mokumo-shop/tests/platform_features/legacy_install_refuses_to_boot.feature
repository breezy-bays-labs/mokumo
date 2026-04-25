Feature: Legacy install refuses to boot when slug derivation would fail

  When the engine inspects an existing `production/` data directory that
  has admin user(s) but a blank `shop_settings.shop_name`, it cannot
  derive a profile slug without inventing one. Refusing to boot leaves
  the operator's data alone and surfaces a clear instruction in the
  log.

  This file owns three cases:
  - empty `shop_name` (failure inside `detect_boot_state`)
  - unparseable `shop_name` like "!!!" (failure inside the upgrade
    handler — `derive_slug` rejects it)
  - reserved-slug `shop_name` like "Demo" (failure inside the upgrade
    handler — `derive_slug` rejects it)

  Scenario: empty shop_name in legacy production DB refuses to boot
    Given a legacy production database with an admin user and an empty shop_name
    When the engine boots
    Then the engine refuses to boot with DefensiveEmptyShopName pointing at the production db

  Scenario: unparseable shop_name in legacy production DB refuses to boot
    Given a legacy production database with an admin user and shop_name "!!!"
    When the engine boots
    Then the engine refuses to boot because the legacy shop_name is unparseable

  Scenario: reserved-slug shop_name in legacy production DB refuses to boot
    Given a legacy production database with an admin user and shop_name "Demo"
    When the engine boots
    Then the engine refuses to boot because the derived slug is reserved "demo"
