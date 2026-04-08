Feature: PIN-Based Password Recovery

  When an admin forgets their password, they can request a recovery file to be
  placed on the server's Desktop. The file contains a 6-digit PIN valid for
  15 minutes. The PIN is never returned in the API response — it must be read
  from the file on the local machine.

  Scenario: Valid email triggers PIN generation and recovery file placement
    Given an admin user exists
    When the user requests a password reset via file drop
    Then a recovery file is placed on the user's Desktop
    And the file contains a PIN for resetting the password

  Scenario: Unknown email is rejected
    When the user requests a password reset for an unknown email
    Then the reset is rejected

  Scenario: Valid PIN resets password and clears the pending reset
    Given a recovery PIN has been generated
    When the user enters the correct PIN with a new password
    Then the password is updated
    And the recovery PIN is invalidated

  Scenario: Invalid PIN is rejected but the valid PIN remains usable
    Given a recovery PIN has been generated
    When the user enters an incorrect PIN
    Then the reset is rejected
    And the valid PIN remains usable

  Scenario: Expired PIN is rejected
    Given a recovery PIN was generated more than 15 minutes ago
    When the user enters the PIN with a new password
    Then the reset is rejected as expired

  Scenario: Reset attempt with no prior request is rejected
    Given an admin user exists
    When the user enters an incorrect PIN
    Then the reset is rejected

  @wip
  Scenario: Excessive reset requests are rate limited
    Given an admin user exists
    When the user makes 6 forgot-password requests within 15 minutes
    Then the 6th request is rejected
