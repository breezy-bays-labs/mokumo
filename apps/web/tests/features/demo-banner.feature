@wip
Feature: Demo mode banner

  When Mokumo is running with demo data, a persistent banner
  tells the user they are exploring demo data and offers a
  link to Settings.

  Scenario: Banner appears in demo mode
    Given the server is running in demo mode
    When the app shell loads
    Then a demo banner is visible
    And the banner text says "You're exploring demo data"

  Scenario: Banner contains a link to Settings
    Given the demo banner is visible
    Then the banner contains a "Go to Settings" link

  Scenario: Settings link navigates to System Settings
    Given the demo banner is visible
    When I click "Go to Settings"
    Then I am on the System Settings page

  Scenario: Banner can be dismissed
    Given the demo banner is visible
    When I click the dismiss button on the banner
    Then the demo banner is no longer visible

  Scenario: Banner dismissal persists across page loads
    Given I have dismissed the demo banner
    When I refresh the page
    Then the demo banner is not visible

  Scenario: Demo reset clears banner dismissal state
    Given I have dismissed the demo banner
    When the demo data is reset
    Then the banner dismissal state is cleared

  Scenario: Banner is visible after app reloads post-reset
    Given the demo data has been reset
    When the app reloads
    Then the demo banner is visible

  Scenario: Banner does not appear in production mode
    Given the server is running in production mode
    When the app shell loads
    Then no demo banner is visible

  Scenario: Banner does not appear on fresh install
    Given the server has not completed setup
    When the app redirects to the setup wizard
    Then no demo banner is visible
