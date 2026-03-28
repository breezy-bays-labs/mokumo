Feature: Low-count recovery code warning banner

  A persistent warning banner appears in the app shell when the admin
  has fewer than 3 recovery codes remaining. The banner is dismissable
  per session and reappears on the next login.

  # --- Banner visibility ---

  Scenario: Banner appears when codes are below threshold
    Given the admin has 2 recovery codes remaining
    When the admin navigates to any app page
    Then an amber warning banner is visible
    And the banner text mentions the remaining code count
    And the banner contains a link to Settings Account

  Scenario: Banner does not appear when codes are above threshold
    Given the admin has 5 recovery codes remaining
    When the admin navigates to any app page
    Then no recovery code warning banner is visible

  Scenario: Banner does not appear when codes are at threshold
    Given the admin has 3 recovery codes remaining
    When the admin navigates to any app page
    Then no recovery code warning banner is visible

  Scenario: Banner appears when all codes are exhausted
    Given the admin has 0 recovery codes remaining
    When the admin navigates to any app page
    Then an amber warning banner is visible
    And the banner text mentions zero remaining codes

  # --- Dismiss behavior ---

  Scenario: Dismiss hides the banner for the current session
    Given the recovery code warning banner is visible
    When the admin clicks the dismiss button
    Then the banner is hidden
    And navigating to another page does not show the banner

  Scenario: Banner reappears on next session
    Given the admin previously dismissed the warning banner
    When the admin logs in again in a new session
    Then the warning banner is visible again
