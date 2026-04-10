Feature: Unsaved changes guard on page navigation

  If a user navigates away from a page with unsaved form changes,
  an "Unsaved Changes" dialog blocks the navigation. The user must
  confirm ("Leave anyway") or cancel. Confirming completes the
  navigation and discards the changes. Canceling keeps them on the
  current page with form data intact.

  # --- Clean path (no dirty forms) ---

  Scenario: Navigation proceeds immediately with no unsaved changes
    Given I am on the customers page with no dirty forms
    When I click a sidebar link to navigate away
    Then the navigation completes without a dialog

  # --- Dirty path ---

  Scenario: Unsaved changes dialog appears on navigation with dirty form
    Given I have unsaved changes in the customer form
    When I click a sidebar link to navigate away
    Then the "Unsaved changes" navigation dialog appears
    And the navigation has not completed

  Scenario: "Leave anyway" allows navigation to proceed
    Given the navigation unsaved changes dialog is open
    When I click "Leave anyway" in the navigation dialog
    Then the dialog closes
    And the navigation completes to the destination

  Scenario: "Cancel" keeps user on current page
    Given the navigation unsaved changes dialog is open
    When I click "Cancel" in the navigation dialog
    Then the dialog closes
    And I remain on the customers page with form data intact

  # --- Back button ---

  Scenario: Browser back button triggers guard when form is dirty
    Given I navigated to the customers page from the home page
    And I have unsaved changes in the customer form
    When I press the browser back button
    Then the "Unsaved changes" navigation dialog appears

  # --- Clean after save ---

  Scenario: Guard does not trigger after successful save
    Given I have unsaved changes in the customer form
    When I save the customer form successfully
    And I click a sidebar link to navigate away
    Then the navigation completes without a dialog
