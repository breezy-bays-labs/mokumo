@wip
Feature: Data Safety card in Settings

  After any schema upgrade, Mokumo creates a pre-migration backup of
  the shop's database. The Data Safety card in Settings surfaces these
  backups so shop owners can find them and understand how to restore.

  Production backups are emphasized (real shop data). Demo backups are
  de-emphasized with a nudge to set up a production profile.

  # --- Card visibility and navigation ---

  Scenario: Data Safety card is accessible from the Settings navigation
    Given I am logged in as an admin
    When I click the Settings link in the navigation
    Then I am on the Settings page
    And I see the "Data Safety" card

  # --- Production backups ---

  Scenario: Production section shows backup entries when backups exist
    Given the backup-status API returns one production backup
    When I navigate to the Settings page
    Then I see the production backup path
    And I see the backup version label
    And I see the backup timestamp

  Scenario: Production section shows empty state when no backups exist
    Given the backup-status API returns no production backups
    When I navigate to the Settings page
    Then I see "No backups yet" in the production section
    And I see "A backup will be taken automatically before your next upgrade"

  Scenario: Production backups are displayed newest first
    Given the backup-status API returns three production backups
    When I navigate to the Settings page
    Then the most recent backup appears at the top of the production list

  Scenario: Restore instructions are shown when a production backup exists
    Given the backup-status API returns one production backup
    When I navigate to the Settings page
    Then I see restore instructions in the production section
    And the instructions include "quit Mokumo"
    And the instructions include the backup filename

  # --- Demo backups ---

  Scenario: Demo section is de-emphasized and shows a production nudge
    Given the server is running in demo mode
    And the backup-status API returns one demo backup
    When I navigate to the Settings page
    Then I see the demo section
    And I see "You're viewing demo data" nudge copy
    And I see "Set up your production profile to protect your real shop data"

  Scenario: Demo section shows empty state when no demo backups exist
    Given the backup-status API returns no demo backups
    When I navigate to the Settings page
    Then I see "No backups yet" in the demo section

  # --- One-time upgrade toast ---

  Scenario: Upgrade toast appears on first boot after upgrade when backups exist
    Given this is the first load for the current app version
    And the backup-status API returns at least one backup
    When I navigate to any page in the app
    Then I see an upgrade toast notification
    And the toast contains a "View backups" link to the Settings page

  Scenario: Upgrade toast does not appear if already dismissed for this version
    Given the upgrade toast has already been dismissed for the current version
    When I navigate to any page in the app
    Then no upgrade toast is shown

  Scenario: Upgrade toast does not appear on fresh install with no backups
    Given this is the first load for the current app version
    And the backup-status API returns no backups for any profile
    When I navigate to any page in the app
    Then no upgrade toast is shown

  # --- Server startup error (restart-loop path) ---

  Scenario: Migration failure toast includes backup path when backup was taken
    Given the Tauri "server-error" event fires with a migration_failed error
    And the error includes a backup_path
    When the error event is received
    Then I see a persistent error toast
    And the toast message includes "backed up at"
    And the toast message includes the backup path

  Scenario: Migration failure toast omits backup path when no backup was taken
    Given the Tauri "server-error" event fires with a migration_failed error
    And the error has no backup_path
    When the error event is received
    Then I see a persistent error toast
    And the toast message does not include "backed up at"

  @wip
  Scenario: Logo is preserved across backup and restore
    Given I am on the production profile
    And a logo has been uploaded
    When I create a backup
    And I delete the logo
    And I restore from the backup
    Then the sidebar profile trigger shows the custom logo
    And GET /api/shop/logo returns 200
