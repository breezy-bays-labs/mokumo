Feature: Recovery code regeneration from Settings

  The Settings > Account page shows how many recovery codes remain
  and provides a regeneration action gated by password confirmation.
  After regeneration, codes are displayed once with save affordances.

  # --- Code count display ---

  Scenario: Settings Account shows remaining code count
    Given the admin is on the Settings Account page
    Then the page shows the remaining recovery code count
    And a "Regenerate Recovery Codes" button is visible

  # --- Regeneration flow ---

  Scenario: Clicking Regenerate opens a confirmation dialog
    Given the admin is on the Settings Account page
    When the admin clicks "Regenerate Recovery Codes"
    Then a confirmation dialog appears
    And the dialog contains a password input field
    And the dialog shows a destructive warning message
    And a "Regenerate" confirmation button is visible
    And a "Cancel" button is visible

  Scenario: Cancel closes the dialog without regenerating
    Given the confirmation dialog is open
    When the admin clicks "Cancel"
    Then the dialog closes
    And no codes are regenerated

  Scenario: Correct password regenerates and displays new codes
    Given the confirmation dialog is open
    When the admin enters the correct password and confirms
    Then the dialog closes
    And the page displays 10 new recovery codes
    And download and print buttons are visible
    And an "I have saved my codes" checkbox is visible
    And a "Done" button is visible but disabled

  Scenario: Wrong password shows an error in the dialog
    Given the confirmation dialog is open
    When the admin enters an incorrect password and confirms
    Then the dialog shows an error message
    And the dialog remains open

  # --- Save gate ---

  Scenario: Done button is enabled after checking the save checkbox
    Given new codes are displayed on the page
    When the admin checks "I have saved my codes"
    Then the "Done" button becomes enabled

  Scenario: Clicking Done returns to the default view with updated count
    Given new codes are displayed and the save checkbox is checked
    When the admin clicks "Done"
    Then the page returns to the default view
    And the recovery code count shows 10 of 10
