Feature: Help popover in sidebar footer

  The sidebar footer contains a help icon that opens a popover with a link
  to the external demo guide. The help icon is separate from the nav-items
  contract — it triggers an external browser navigation, not SPA routing.

  # --- Visibility ---

  Scenario: Help icon is visible in the sidebar footer
    Given the app shell is loaded
    Then the sidebar footer displays a help icon before the user avatar

  Scenario: Help icon shows tooltip on hover when collapsed
    Given the sidebar is collapsed to icon rail
    When I hover over the help icon
    Then a tooltip shows "Help"

  Scenario: Help icon is visible when sidebar is collapsed
    Given the sidebar is collapsed to icon rail
    Then the help icon is still visible in the footer

  # --- Popover ---

  Scenario: Clicking the help icon opens a popover
    Given the app shell is loaded
    When I click the help icon
    Then a help popover appears
    And the popover heading is "Demo Guide"
    And the popover contains an "Open Demo Guide" button
    And the popover shows a "Requires internet" note

  Scenario: Popover closes when clicking outside
    Given the help popover is open
    When I click outside the popover
    Then the help popover closes

  Scenario: Popover closes on Escape key
    Given the help popover is open
    When I press Escape on the help popover
    Then the help popover closes

  # --- External Navigation ---

  Scenario: Open Demo Guide button opens the guide in a new tab
    Given the help popover is open
    When I click the Open Demo Guide link
    Then a new browser tab opens with the demo guide URL
