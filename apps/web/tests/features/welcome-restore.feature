@wip
Feature: Welcome screen — Open Existing Shop

  On a fresh install, the welcome screen offers "Open Existing Shop" as a
  third option. This lets a shop owner restore from an existing .db backup
  file instead of starting fresh or exploring the demo.

  Playwright BDD step definitions are tracked as follow-up #464.
  All scenarios are tagged @wip (file-level) until steps are implemented.

  # --- Welcome screen CTA ---

  Scenario: Welcome screen shows "Open Existing Shop" button
    Given I am on the welcome screen
    Then I see an "Open Existing Shop" button
    And the "Open Existing Shop" button has secondary/outline styling

  Scenario: "Open Existing Shop" is the third button
    Given I am on the welcome screen
    Then the button order is "Set Up My Shop", "Explore Demo", "Open Existing Shop"

  Scenario: All buttons are disabled while any action is in flight
    Given I am on the welcome screen
    When I click "Open Existing Shop"
    Then all three buttons are disabled

  # --- File picker ---

  Scenario: Clicking "Open Existing Shop" opens the file picker
    Given I am on the welcome screen
    When I click "Open Existing Shop"
    Then a file picker dialog opens filtered to .db files

  Scenario: Cancelling the file picker returns to welcome screen
    Given the file picker is open from "Open Existing Shop"
    When I cancel the file picker
    Then I am on the welcome screen

  # --- Validation phase ---

  Scenario: Selected file is validated before confirmation
    Given I selected a .db file via the file picker
    Then I see a "Validating your database..." message with a spinner
    And a validation request is sent to the server

  Scenario: Valid file shows confirmation screen
    Given I selected a valid Mokumo .db file
    When validation succeeds
    Then I see the file name and size
    And I see "Valid Mokumo database" with a success indicator
    And I see a credential warning "You'll need your login credentials from this database"
    And I see an "Import and Restart" button

  Scenario: Invalid file shows error with guidance
    Given I selected a non-Mokumo .db file
    When validation fails with "not_mokumo_database"
    Then I see an error message explaining the file is not a valid Mokumo database
    And I see a "Choose Different File" button
    And I see a "Back" link

  Scenario: Corrupt file shows error with guidance
    Given I selected a corrupt .db file
    When validation fails with "database_corrupt"
    Then I see an error message explaining the file appears damaged
    And I see a "Choose Different File" button

  Scenario: Incompatible schema shows update guidance
    Given I selected a .db file from a newer Mokumo version
    When validation fails with "schema_incompatible"
    Then I see an error message advising to update Mokumo

  Scenario: "Choose Different File" re-opens the file picker
    Given validation has failed for a selected file
    When I click "Choose Different File"
    Then the file picker opens again

  # --- Import phase ---

  Scenario: "Import and Restart" triggers the restore
    Given I see the confirmation screen with a valid file
    When I click "Import and Restart"
    Then I see a spinner with "Importing your shop data..."
    And a restore request is sent to the server

  Scenario: Import failure shows error with recovery options
    Given I clicked "Import and Restart"
    When the restore request fails
    Then I see an error message
    And I see a "Try Again" button
    And I see a "Back to Welcome" link

  Scenario: "Try Again" after import failure re-opens file picker
    Given the import has failed
    When I click "Try Again"
    Then the file picker opens again

  # --- Restart phase ---

  Scenario: Successful import shows restarting state then redirects to login
    Given I clicked "Import and Restart"
    When the restore request succeeds
    Then I see "Restarting server..."
    And the page reloads to "/login" after a short delay

  Scenario: Server restart timeout shows manual restart guidance
    Given the restore succeeded and the server is restarting
    When the server does not respond within 15 seconds
    Then I see "Server did not restart. Please restart Mokumo manually."
    And I see a "Retry" button

  # --- Post-restore login banner ---

  Scenario: Login page shows restore banner after successful import
    Given a shop database was just restored
    When I arrive at "/login?restored=true"
    Then I see a banner "Your shop data has been imported. Sign in with your existing credentials."

  Scenario: Restore banner is dismissible
    Given I see the restore banner on the login page
    When I dismiss the banner
    Then the banner is no longer visible

  # --- Direct navigation guard ---

  Scenario: Direct navigation to restore screen without file redirects to welcome
    Given no file has been selected
    When I navigate directly to "/welcome/restore"
    Then I am redirected to "/welcome"

  # --- Rate limiting ---

  Scenario: Rate limit error shows waiting message
    Given I have exceeded the import attempt limit
    When I try to validate or import another file
    Then I see "Too many import attempts. Please wait before trying again."
