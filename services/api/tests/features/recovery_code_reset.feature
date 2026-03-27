Feature: Recovery Code Password Reset

  Recovery codes are generated during setup as a secondary password
  reset mechanism. Each code is single-use. When all codes are
  exhausted, the CLI reset-password command is the final fallback.

  Scenario: Valid recovery code resets the password
    Given an admin user has unused recovery codes
    When the user enters a valid recovery code with a new password
    Then the password is updated
    And the recovery code is marked as used
    And the password change is recorded in the activity log

  Scenario: Used recovery code is rejected
    Given an admin user has already used a recovery code
    When the user enters the same recovery code again
    Then the reset is rejected

  Scenario: Invalid recovery code is rejected
    Given an admin user exists
    When the user enters a code that was never issued
    Then the reset is rejected

  Scenario: All recovery codes exhausted
    Given an admin user has used all 10 recovery codes
    When the user attempts to reset with a recovery code
    Then the reset is rejected
    And no recovery codes remain available

  @wip
  Scenario: Excessive recovery attempts are rate limited
    Given an admin user has unused recovery codes
    When the user makes 6 recovery code attempts within 15 minutes
    Then the 6th attempt is rejected
    And the rejection is indistinguishable from an invalid code response
