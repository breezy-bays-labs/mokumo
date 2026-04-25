Feature: Boot-state detection

  Before the engine opens per-profile pools, it inspects the data
  directory and decides which boot path applies. There are exactly
  five boot states; each maps to a distinct dispatcher action in
  `Engine::boot`. See M00 shape doc §Seam 1.

  - FreshInstall                    → run setup wizard
  - PostUpgradeOrSetup              → normal boot
  - LegacyAbandoned (NoVerticalDb)  → log + treat as fresh install
  - LegacyAbandoned (NoAdminUser)   → log + treat as fresh install
  - LegacyCompleted                 → silent legacy upgrade (A1.2)
  - LegacyDefensiveEmpty            → refuse to boot

  Background:
    Given a fresh data directory
    And a meta pool with the profiles table created

  Scenario: fresh install when meta is empty and no production folder
    When boot-state detection runs
    Then the boot state is FreshInstall

  Scenario: post-upgrade-or-setup when meta.profiles has rows
    Given meta.profiles has 2 rows
    When boot-state detection runs
    Then the boot state is PostUpgradeOrSetup with profile_count 2

  Scenario: legacy abandoned when production folder exists without vertical DB
    Given a legacy production folder with no vertical DB
    When boot-state detection runs
    Then the boot state is LegacyAbandoned with reason NoVerticalDbFile

  Scenario: legacy abandoned when vertical DB has no admin user
    Given a legacy production folder with a vertical DB that has no admin user
    When boot-state detection runs
    Then the boot state is LegacyAbandoned with reason NoAdminUser

  Scenario: legacy completed install eligible for upgrade
    Given a legacy production folder with an admin user and shop_name "Acme Printing"
    When boot-state detection runs
    Then the boot state is LegacyCompleted with shop_name "Acme Printing"

  Scenario: legacy defensive empty refuses to boot when shop_name is blank
    Given a legacy production folder with an admin user and shop_name ""
    When boot-state detection runs
    Then the boot state is LegacyDefensiveEmpty
