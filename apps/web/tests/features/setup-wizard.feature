Feature: Setup wizard onboarding experience

  The setup wizard guides a new shop owner through first-time
  configuration: welcome, shop name, admin account, recovery codes,
  and a completion screen with LAN connection details.

  # --- Setup Token Visibility ---

  Scenario: Setup token field is hidden when launched from the desktop app
    Given the setup wizard is opened with a setup token in the URL
    When I reach the admin account step
    Then I do not see the setup token field
    And the admin account form shows name, email, and password fields

  Scenario: Setup token field is visible for CLI users
    Given the setup wizard is opened without a setup token in the URL
    When I reach the admin account step
    Then I see the setup token field
    And the field helper text says "Find this in the terminal where you started Mokumo."

  Scenario: Setup token field is revealed when account creation fails
    Given the setup wizard is opened with a setup token in the URL
    When I reach the admin account step
    And account creation fails with an error
    Then I see the setup token field
    And I see the error message

  # --- Completion Screen ---

  Scenario: Completion screen shows the LAN URL
    Given the server has mDNS active with hostname "mokumo.local" on port 3000
    And I have completed the setup wizard
    When I reach the completion screen
    Then I see "You're all set!"
    And I see the LAN URL "http://mokumo.local:3000"
    And I see instructions for connecting other devices

  Scenario: Completion screen LAN URL can be copied
    Given the server has mDNS active with hostname "mokumo.local" on port 3000
    And I am on the setup completion screen
    When I copy the LAN URL
    Then the clipboard contains "http://mokumo.local:3000"
    And I see a "URL copied to clipboard" toast message

  Scenario: Completion screen navigates to the dashboard
    Given I am on the setup completion screen
    When I click "Open Dashboard"
    Then I am redirected to the dashboard

  # --- Edge Cases ---

  @future
  Scenario: Completion screen without LAN access
    Given the server has no LAN access
    And I have completed the setup wizard
    When I reach the completion screen
    Then I see "You're all set!"
    And I do not see a LAN URL section
