Feature: Sidebar profile switcher

  The top-left sidebar header contains a profile switcher. Clicking the
  shop name / wordmark opens a dropdown with both profiles listed. The
  active profile has a checkmark. Selecting the inactive profile calls
  POST /api/profile/switch and reloads the app shell.

  # --- Trigger ---

  Scenario: Sidebar header shows the active profile name
    Given I am on the demo profile
    When the app shell loads
    Then the sidebar header shows "Mokumo Software"

  Scenario: Sidebar header shows shop name on production profile
    Given I am on the production profile with shop name "Gary's Printing Co"
    When the app shell loads
    Then the sidebar header shows "Gary's Printing Co"

  Scenario: Clicking the sidebar header opens the profile dropdown
    Given the app shell is loaded
    When I click the sidebar header trigger
    Then the profile dropdown opens

  Scenario: Clicking the sidebar header again closes the dropdown
    Given the profile dropdown is open
    When I click the sidebar header trigger
    Then the profile dropdown closes

  Scenario: Collapsed sidebar does not show the switcher trigger
    Given the sidebar is in collapsed/icon-only mode
    Then the profile switcher trigger text and chevron are hidden
    And only the logo icon is visible

  # --- Dropdown content ---

  Scenario: Dropdown lists demo profile entry with DEMO badge
    Given the profile dropdown is open
    Then I see an entry for "Mokumo Software"
    And that entry has a "DEMO" badge

  Scenario: Dropdown lists production profile entry when set up
    Given the profile dropdown is open
    And production setup has been completed with shop name "Gary's Printing Co"
    Then I see an entry for "Gary's Printing Co"
    And that entry has no badge

  Scenario: Dropdown shows "Set Up My Shop" when production is not yet configured
    Given the profile dropdown is open
    And production setup has not been completed
    Then I see a "Set Up My Shop" entry instead of a production shop name

  Scenario: Active profile entry has a checkmark
    Given I am on the demo profile
    And the profile dropdown is open
    Then the "Mokumo Software" entry has a checkmark indicator
    And the production entry has no checkmark

  # --- Switching profiles ---

  Scenario: Selecting the inactive profile triggers a switch
    Given I am on the demo profile
    And the profile dropdown is open
    When I click the "Gary's Printing Co" production entry
    Then a profile switch request is sent for the production profile
    And the app reloads to "/"
    And the sidebar header now shows "Gary's Printing Co"

  Scenario: Selecting the active profile does nothing
    Given I am on the demo profile
    And the profile dropdown is open
    When I click the "Mokumo Software" demo entry
    Then no profile switch request is sent
    And the dropdown closes

  Scenario: Loading indicator appears on the entry while switch is in flight
    Given the profile dropdown is open
    When I click a profile entry
    Then a spinner appears on that entry
    And both entries are disabled

  Scenario: Dropdown closes after a successful switch
    Given I triggered a profile switch from the dropdown
    When the switch completes successfully
    Then the dropdown is closed

  # --- Opened by external triggers ---

  Scenario: Demo banner CTA opens the profile switcher dropdown
    Given the demo banner is visible
    When I click the banner CTA
    Then the sidebar profile switcher dropdown opens automatically

  @future
  Scenario: Settings shortcut opens the profile switcher dropdown
    Given I am on the System Settings page
    When I click "Open Profile Switcher"
    Then the sidebar profile switcher dropdown opens automatically
