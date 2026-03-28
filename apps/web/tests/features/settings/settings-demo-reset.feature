@wip
Feature: Demo reset in Settings

  The System Settings page shows a demo reset action when running
  in demo mode. Resetting replaces the demo database with a fresh
  copy and restarts the app.

  Scenario: Reset button appears in demo mode
    Given the server is running in demo mode
    When I navigate to the System Settings page
    Then I see a "Reset Demo Data" button
    And I see a demo mode indicator

  Scenario: Reset button does not appear in production mode
    Given the server is running in production mode
    When I navigate to the System Settings page
    Then I do not see a "Reset Demo Data" button
    And I do not see a demo mode indicator

  Scenario: Clicking reset shows a confirmation dialog
    Given I am on the System Settings page in demo mode
    When I click "Reset Demo Data"
    Then a confirmation dialog appears
    And the dialog warns "This will erase all changes to demo data"

  Scenario: Confirming reset triggers the reset flow
    Given the reset confirmation dialog is open
    When I click the "Reset" button
    Then a reset request is sent to the server
    And I see a progress indicator

  Scenario: Canceling reset closes the dialog
    Given the reset confirmation dialog is open
    When I click "Cancel"
    Then the dialog closes
    And I am still on the System Settings page
