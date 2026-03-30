@wip
Feature: Demo mode banner

  When Mokumo is running with demo data, a permanent banner tells the
  user they are in demo mode and offers a CTA to open the profile
  switcher. The banner cannot be dismissed.

  # --- Visibility ---

  Scenario: Banner appears in demo mode
    Given the server is running in demo mode
    When the app shell loads
    Then the demo banner is visible

  Scenario: Banner does not appear in production mode
    Given the server is running in production mode
    When the app shell loads
    Then no demo banner is visible

  # --- Not dismissible ---

  Scenario: Banner has no dismiss button
    Given the demo banner is visible
    Then there is no dismiss or close button on the banner

  Scenario: Banner remains visible after page navigation
    Given the demo banner is visible
    When I navigate to the Customers page
    Then the demo banner is still visible

  Scenario: Banner remains visible after page reload
    Given the demo banner is visible
    When I reload the page
    Then the demo banner is visible

  # --- CTA text (context-sensitive) ---

  Scenario: Banner shows "Set Up My Shop" before production is configured
    Given the server is running in demo mode
    And production setup has not been completed
    When the app shell loads
    Then the banner CTA reads "Set Up My Shop"

  Scenario: Banner shows "Go to My Shop" after production is configured
    Given the server is running in demo mode
    And production setup has been completed
    When the app shell loads
    Then the banner CTA reads "Go to My Shop"

  # --- CTA action ---

  Scenario: Clicking the banner CTA opens the sidebar profile switcher
    Given the demo banner is visible
    When I click the banner CTA button
    Then the sidebar profile switcher dropdown opens

  Scenario: Banner CTA does not navigate to Settings
    Given the demo banner is visible
    When I click the banner CTA button
    Then I am still on the same page
    And I have not been navigated to Settings
