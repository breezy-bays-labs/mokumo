Feature: First-run setup wizard

  The setup wizard guides a brand-new operator through the
  smallest set of steps needed to bring an admin account, a first
  profile, and the public shop URL online. It can be entered from
  either the desktop app (with a setup token already in the URL)
  or a CLI-launched browser session (where the operator pastes the
  token themselves), and it survives interruption — leaving the
  page mid-wizard does not strand the operator.

  # --- Wizard structure ---

  @pr2a
  Scenario: Setup wizard presents four steps
    Given the setup wizard is opened with a valid setup token
    Then I see a four-step progress indicator
    And the steps are "Welcome", "Create admin", "Create profile", and "Finish"

  @pr2a
  Scenario: Welcome step folds the token verification into the greeting
    Given the setup wizard is opened with a valid setup token
    When I am on the welcome step
    Then I see a welcome message
    And I see that the setup token has already been accepted
    And I do not see a separate "Verify token" step

  # --- Setup token visibility (absorbed from apps/web/setup-wizard.feature) ---

  @pr2a
  Scenario: Setup token field is hidden when launched from the desktop app
    Given the setup wizard is opened with a setup token in the URL
    When I reach the create-admin step
    Then I do not see the setup token field
    And the create-admin form shows name, email, and password fields

  @pr2a
  Scenario: Setup token field is visible for CLI users
    Given the setup wizard is opened without a setup token in the URL
    When I reach the create-admin step
    Then I see the setup token field
    And the field helper text tells me where to find the token in my terminal

  @needs-pr2b
  Scenario: Setup token field is revealed when account creation fails
    Given the setup wizard is opened with a setup token in the URL
    When I reach the create-admin step
    And account creation fails with an error
    Then I see the setup token field
    And I see the error message

  # --- Create-profile step ---

  @needs-pr2b
  Scenario: Create-profile step shows the resolved profile location
    Given I have completed the create-admin step
    When I reach the create-profile step
    And I enter a profile name
    Then I see a "Your shop lives at" fact strip
    And the fact strip shows the resolved profile directory path

  # --- Finish step ---

  @needs-pr2b
  Scenario: Finish step shows the shop URL on the local network
    Given the platform reports an mDNS hostname and port for this shop
    And I have completed every prior wizard step
    When I reach the finish step
    Then I see a "You're all set!" headline
    And I see the shop URL on the local network
    And I see instructions for connecting other devices

  @pr2a
  Scenario: Finish step lets me copy the shop URL
    Given I am on the finish step
    And the platform reports an mDNS hostname and port
    When I copy the shop URL
    Then the clipboard contains the shop URL
    And I see a "URL copied to clipboard" toast

  @needs-pr2b
  Scenario: Finish step navigates me into the admin overview
    Given I am on the finish step
    When I click "Open Dashboard"
    Then I am taken to the admin overview

  # --- Resume on abandonment ---

  @needs-pr2b @spans-layers
  Scenario: Returning to setup after walking away resumes at the next unfinished step
    Given I started the setup wizard and completed the create-admin step
    And I closed the browser without finishing
    When I open the setup wizard again
    Then the wizard resumes on the create-profile step
    And my admin account is not duplicated

  @needs-pr2b
  Scenario: Starting over from a resumed wizard prompts for confirmation
    Given the setup wizard has resumed on the create-profile step
    When I click "Start over"
    Then I am asked to confirm that I want to discard the existing admin account
    And nothing is changed until I confirm

  # --- Cancel and leave ---

  @pr2a
  Scenario: Trying to leave mid-wizard prompts a "Leave setup?" confirmation
    Given I am on the create-profile step
    When I try to navigate away from the wizard
    Then I am asked "Leave setup?"
    And I can choose to stay on the wizard

  # --- Failure modes ---

  @pr2a
  Scenario: Offline during the wizard shows a self-healing reconnecting banner
    Given I am on the create-profile step
    And I have entered a profile name
    And the platform becomes unreachable
    Then I see a self-healing banner that the connection is being retried
    And my form values are preserved

  @needs-pr2b
  Scenario: Invalid setup token rejected with actionable error
    Given the setup wizard is opened with an invalid setup token
    When I land on the welcome step
    Then I see a clear "Setup token is not valid" message
    And I am told how to obtain a fresh token
