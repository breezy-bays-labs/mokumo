Feature: Unsaved changes guard on profile switch

  If a user initiates a profile switch while a form has unsaved changes,
  an "Unsaved Changes" dialog interrupts the switch. The user must
  confirm ("Leave anyway") or cancel. Confirming completes the switch.
  Canceling returns them to the form with no changes made.

  # --- Clean path (no dirty forms) ---

  Scenario: Profile switch proceeds immediately with no unsaved changes
    Given I am on a page with no dirty forms
    When I select a different profile from the sidebar switcher
    Then no unsaved changes dialog appears
    And the profile switch proceeds immediately

  # --- Dirty path ---

  Scenario: Unsaved changes dialog appears when form is dirty
    Given I have unsaved changes in a form
    When I select a different profile from the sidebar switcher
    Then the "Unsaved changes" dialog appears
    And the profile switch has not been sent yet

  Scenario: Dialog shows the correct warning text
    Given the unsaved changes dialog is open
    Then I see text "You have unsaved changes that will be lost"

  Scenario: "Leave anyway" completes the profile switch
    Given the unsaved changes dialog is open
    When I click "Leave anyway"
    Then the profile switch request is sent
    And the dialog closes
    And the app navigates to the new profile

  Scenario: "Cancel" closes the dialog without switching
    Given the unsaved changes dialog is open
    When I click "Cancel"
    Then the dialog closes
    And no profile switch request has been sent
    And I am still on the same page with my form data intact

  Scenario: Pressing Escape cancels the switch (same as Cancel)
    Given the unsaved changes dialog is open
    When I press the Escape key
    Then the dialog closes
    And no profile switch request has been sent

  Scenario: Clicking outside the dialog does not dismiss it
    Given the unsaved changes dialog is open
    When I click outside the dialog
    Then the "Unsaved changes" dialog remains open
    And no profile switch request has been sent

  # --- Form dirty state tracking ---

  Scenario: Form with typed but unsaved input is considered dirty
    Given I navigate to a page with a form
    When I type in an input field without saving
    Then the form is tracked as dirty

  Scenario: Form is no longer dirty after successful save
    Given I have unsaved changes in a form
    When I save the form
    Then the form is no longer tracked as dirty
    And profile switching proceeds without the warning dialog

  Scenario: Form is no longer dirty after navigating away and returning
    Given I had unsaved changes and navigated away
    When I return to that form
    Then the form is not considered dirty (changes were abandoned)

  # --- Edge cases ---

  Scenario: Multiple pending changes on the same form trigger the dialog only once
    Given I have two open forms with unsaved changes
    When I initiate a profile switch
    Then the unsaved changes dialog appears once
    And clicking "Leave anyway" switches the profile

  Scenario: Switch from Settings shortcut also triggers dirty check
    Given I have unsaved changes in a form
    When I click "Open Profile Switcher" on the Settings page
    And I select a different profile
    Then the unsaved changes dialog appears

  # --- Error handling on the dirty path ---

  Scenario: Rate-limited response during dirty-forms confirmation surfaces the server error message
    Given the unsaved changes dialog is open
    And the profile switch API returns a rate_limited error
    When I click "Leave anyway"
    Then a toast appears containing "Too many"
    And the unsaved changes dialog is still open
