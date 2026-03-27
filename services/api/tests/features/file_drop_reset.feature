Feature: File-Drop Password Reset

  When a shop owner forgets their password, Mokumo places a recovery
  file on the Desktop containing a short-lived PIN. The owner enters
  the PIN to prove physical access to the machine and sets a new password.

  Scenario: Recovery file is placed on the Desktop
    Given an admin user exists
    When the user requests a password reset via file drop
    Then a recovery file is placed on the user's Desktop
    And the file contains a PIN for resetting the password

  Scenario: Valid PIN resets the password
    Given a recovery PIN has been generated
    When the user enters the correct PIN with a new password
    Then the password is updated
    And the recovery PIN is invalidated
    And the password change is recorded in the activity log

  Scenario: Expired PIN is rejected
    Given a recovery PIN was generated more than 15 minutes ago
    When the user enters the PIN with a new password
    Then the reset is rejected as expired

  Scenario: Incorrect PIN is rejected
    Given a recovery PIN has been generated
    When the user enters an incorrect PIN
    Then the reset is rejected
    And the valid PIN remains usable
