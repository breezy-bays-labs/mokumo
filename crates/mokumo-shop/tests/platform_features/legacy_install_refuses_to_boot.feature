Feature: Legacy install refuses to boot when slug derivation would fail

  When the engine inspects an existing `production/` data directory that
  has admin user(s) but a blank `shop_settings.shop_name`, it cannot
  derive a profile slug without inventing one. Refusing to boot leaves
  the operator's data alone and surfaces a clear instruction in the
  log.

  This file owns the empty-shop_name case (failure inside
  `detect_boot_state`). Reserved-slug and unparseable-shop_name cases
  land alongside the upgrade handler in A1.2.

  Scenario: empty shop_name in legacy production DB refuses to boot
    Given a legacy production database with an admin user and an empty shop_name
    When the engine boots
    Then the engine refuses to boot with DefensiveEmptyShopName pointing at the production db
