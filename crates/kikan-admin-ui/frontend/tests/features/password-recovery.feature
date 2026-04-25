Feature: Password recovery via local file drop

  When an admin forgets their password they recover it without
  email or external services. The platform writes a recovery PIN
  to a local recovery directory the operator can read; the
  operator pastes the PIN into the recovery wizard and chooses a
  new password. The flow is three steps and never invents secrets
  the operator hasn't seen.

  # --- Wizard structure ---

  @pr2a
  Scenario: Recovery wizard presents three steps
    Given I open the password-recovery wizard
    Then I see a three-step progress indicator
    And the steps are "Request PIN", "Enter PIN", and "New password"

  # --- Step 1: request a PIN ---

  @needs-pr2b
  Scenario: Requesting a PIN tells me where to look for it
    Given I am on the request-PIN step
    When I enter the admin email and submit
    Then I am told a PIN file has been written to the recovery directory
    And I see the resolved recovery directory path
    And I am advanced to the enter-PIN step

  @needs-pr2b
  Scenario: Recovery directory not writable surfaces a clear failure
    Given the recovery directory cannot be written
    When I submit the request-PIN step
    Then I see an error explaining that the recovery directory is not writable
    And the error names the directory path
    And I am not advanced past the request step

  # --- Step 2: enter the PIN ---

  @needs-pr2b
  Scenario: Wrong PIN keeps me on the enter-PIN step with a generic message
    Given I am on the enter-PIN step
    When I enter a PIN that does not match
    Then I see a generic "PIN is not valid" message
    And I remain on the enter-PIN step

  @needs-pr2b
  Scenario: Expired PIN tells me to request a new one
    Given I am on the enter-PIN step
    And the most recent PIN has expired
    When I enter the expired PIN
    Then I see a "This PIN has expired" message
    And I see a button to request a new PIN

  @needs-pr2b
  Scenario: Repeated bad PINs trigger rate-limiting
    Given I have entered an incorrect PIN five times
    When I submit a sixth attempt
    Then I see a "Too many attempts, try again shortly" message
    And the submit button is disabled until the cooldown ends

  # --- Step 3: new password ---

  @needs-pr2b
  Scenario: Setting a new password takes me to sign-in with a banner
    Given I am on the new-password step with a verified PIN
    When I choose a new password and submit
    Then I am taken to the admin sign-in screen
    And I see a "Password updated" banner

  @pr2a
  Scenario: New password must meet the displayed strength rules
    Given I am on the new-password step with a verified PIN
    When I enter a password that violates a strength rule
    Then I see which rule failed
    And the submit button stays disabled until the rule passes

  # --- Failure paths ---

  @pr2a
  Scenario: Offline during recovery shows a self-healing reconnecting banner
    Given I am on the request-PIN step
    And the platform becomes unreachable
    When I submit the form
    Then I see a self-healing banner that the connection is being retried
    And my form values are preserved
