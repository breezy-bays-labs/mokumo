Feature: Admin overview

  The admin overview is the landing screen after sign-in. On a
  fresh install it teaches the operator what to do next via a
  three-step "Get Started" checklist; once the install is
  configured it shows a four-region dashboard summarizing what's
  happening. The chrome around the overview — sidebar, topbar,
  and branding tokens — is shared by every authenticated screen,
  so this feature is also where the chrome contract is pinned.

  # --- Authentication and install-role gate ---

  @needs-pr2b @spans-layers
  Scenario: Signed-out visitor is redirected to sign-in
    Given I am not signed in
    When I open the admin overview
    Then I am redirected to the admin sign-in screen
    And the original overview path is preserved as a return target

  @needs-pr2b @spans-layers
  Scenario: User without admin install role is redirected to no-admin
    Given I am signed in
    And my install role is not admin
    When I open the admin overview
    Then I am redirected to the no-admin page
    And the no-admin page links me back to the shop

  # --- Fresh install: Get Started checklist ---

  @pr2a
  Scenario: Fresh install shows a three-step Get Started checklist
    Given the platform reports a fresh-install state
    When I open the admin overview
    Then I see a "Get Started" panel
    And I see three checklist steps
    And the steps use the configured app name and shop noun in their copy

  @needs-pr2b
  Scenario: Completing all three Get Started steps swaps the panel for the dashboard
    Given the Get Started checklist has two of three steps completed
    When the final step is completed
    Then I see a "You're set up" completion banner
    And the overview now shows the populated dashboard regions

  @pr2a
  Scenario: The "You're set up" banner auto-dismisses after a short delay
    Given the "You're set up" completion banner is showing
    When the configured display duration elapses
    Then the banner is dismissed automatically
    And the populated dashboard remains visible

  @needs-pr2b
  Scenario: The "You're set up" banner does not reappear on later visits
    Given I have already seen and dismissed the "You're set up" banner
    When I navigate away and return to the overview
    Then I do not see the "You're set up" banner again

  # --- Populated dashboard ---

  @pr2a
  Scenario: Populated overview shows four dashboard regions
    Given the overview is in the populated state
    Then I see a stat strip region
    And I see a recent-activity region
    And I see a backups region
    And I see a system-health region

  @pr2a
  Scenario: Recent activity region links each entry to its source
    Given the overview is in the populated state
    And the recent-activity region lists at least one entry
    When I click a recent-activity entry
    Then I am taken to the screen that owns that entry

  # --- Sidebar (driven by lib/nav.ts) ---

  @pr2a
  Scenario: Sidebar lists every nav entry declared in the nav config
    Given I am on the admin overview
    Then the sidebar lists every entry declared in the nav config
    And each entry's label and href match the nav config

  @pr2a
  Scenario: Sidebar highlights the active route
    Given I am on the admin overview
    Then the overview entry in the sidebar is marked active
    And no other sidebar entry is marked active

  @pr2a
  Scenario: Sidebar shows the PROFILE divider above the profile switcher
    Given I am on the admin overview
    Then I see a "PROFILE" divider in the sidebar
    And the divider sits above the profile switcher block

  @needs-pr2b
  Scenario: Profile switcher shows the active profile as hot and others as greyed
    Given I am signed in with multiple profiles available
    When I open the profile switcher
    Then the active profile is shown as hot
    And every other profile is shown as greyed
    And selecting a greyed profile switches to it

  # --- Topbar ---

  @pr2a
  Scenario: Topbar shows the control-plane label, ADMIN badge, and shop affordances
    Given I am on the admin overview
    Then the topbar shows the "Control Plane" label
    And the topbar shows an "ADMIN" badge
    And the topbar shows an "Open shop" affordance
    And the topbar shows a "Help" affordance

  @pr2a
  Scenario: Topbar does not show a session-elevation countdown
    Given I am on the admin overview
    Then the topbar does not show a countdown timer

  @pr2a
  Scenario: "Open shop" tooltip when no shops are running
    Given I am on the admin overview
    And the platform reports no running shops
    When I hover the "Open shop" affordance
    Then I see a "No shops to open" tooltip
    And the affordance is disabled

  # --- Branding tokens ---

  @pr2a
  Scenario: Branding tokens are surfaced as CSS custom properties on the chrome
    Given the platform reports a branding configuration
    When I open the admin overview
    Then the documented branding CSS custom properties are set on the chrome surfaces
    And the topbar, sidebar, and overview body all consume those tokens

  @pr2a
  Scenario: Sidebar and topbar copy use BrandingConfig nouns, not hard-coded shop words
    Given the platform reports a branding configuration with a custom shop noun
    When I open the admin overview
    Then the sidebar and topbar copy use the configured shop noun
    And no hard-coded mokumo-shop nouns appear in the chrome
