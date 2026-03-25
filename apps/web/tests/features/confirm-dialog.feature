Feature: Confirmation dialog for destructive actions

  A modal dialog that guards destructive actions. Blocks all page interaction
  until the user confirms or cancels. Supports async operations with loading state.

  Scenario: Dialog opens with title and description
    Given Storybook is showing the ConfirmDialog Default story
    When the story renders
    Then the dialog title is visible
    And the dialog description is visible
    And a cancel button is visible
    And an action button is visible

  Scenario: Destructive variant styles the action button
    Given Storybook is showing the ConfirmDialog Destructive story
    When the story renders
    Then the action button has destructive variant styling

  # Skipped: Bits UI Portal creates duplicate dialog instances with independent
  # reactive state in Storybook. Playwright cannot reliably interact with buttons
  # through the portal overlay. Ecosystem-wide gap with no upstream fix.
  # See: https://github.com/cmbays/mokumo/issues/60
  # Radix UI #2845, Headless UI #666, Storybook #21971

  @skip
  Scenario: Cancel closes the dialog
    Given Storybook is showing the ConfirmDialog Default story
    When I click the cancel button
    Then the dialog is no longer visible

  @skip
  Scenario: Confirm shows loading state during async operation
    Given Storybook is showing the ConfirmDialog Loading story
    When confirmation is triggered with a slow operation
    Then the action button shows a loading spinner
    And the cancel button is disabled
    And the action button is disabled

  @skip
  Scenario: Dialog closes after successful confirmation
    Given Storybook is showing the ConfirmDialog Default story
    When confirmation is triggered with a successful operation
    Then the dialog is no longer visible

  @skip
  Scenario: Failed confirmation shows error and keeps dialog open
    Given Storybook is showing the ConfirmDialog Default story
    When confirmation is triggered with a failing operation
    Then an error message is visible in the dialog
    And the dialog is still open

  Scenario: Escape key does not close the dialog
    Given Storybook is showing the ConfirmDialog Default story
    When I press the Escape key
    Then the dialog is still open

  Scenario: Dialog passes accessibility scan
    Given Storybook is showing the ConfirmDialog Default story
    When I run an accessibility scan
    Then no critical accessibility violations are found
