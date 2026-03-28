Feature: Recovery Code Regeneration

  An authenticated admin can regenerate recovery codes from Settings.
  Regeneration requires current password, atomically invalidates all
  previous codes, generates 10 new ones, and logs the action.

  Background:
    Given an admin user exists with recovery codes from setup

  # --- Happy path ---

  Scenario: Valid password regenerates all recovery codes
    Given the admin is logged in
    When the admin submits a regeneration request with the correct password
    Then 10 new recovery codes are returned
    And the codes match the expected format
    And all previous recovery codes are invalidated

  Scenario: Regeneration is atomic with activity logging
    Given the admin is logged in
    When the admin regenerates recovery codes
    Then the activity log contains a "recovery_codes_regenerated" entry
    And the activity actor is the authenticated user

  Scenario: New codes are usable for password reset
    Given the admin is logged in
    And the admin has regenerated recovery codes
    When the admin uses one of the new recovery codes for password reset
    Then the password reset succeeds

  Scenario: Old codes no longer work after regeneration
    Given the admin is logged in
    And the admin has regenerated recovery codes
    When the admin attempts to use an original recovery code
    Then the password reset is rejected

  # --- Password verification ---

  Scenario: Wrong password is rejected
    Given the admin is logged in
    When the admin submits a regeneration request with an incorrect password
    Then the request is rejected with an unauthorized status

  Scenario: Password is re-fetched from database
    Given the admin is logged in
    And the admin has changed their password in another session
    When the admin submits a regeneration request with the old password
    Then the request is rejected with an unauthorized status

  # --- Rate limiting ---

  Scenario: Regeneration is rate limited per user
    Given the admin is logged in
    When the admin makes 4 regeneration requests within an hour
    Then the 4th request is rejected with a rate limit status

  # --- Authentication ---

  Scenario: Unauthenticated request is rejected
    When an unauthenticated request is sent to the regeneration endpoint
    Then the response is 401 Unauthorized

  # --- Code count in /api/auth/me ---

  Scenario: Account status includes remaining code count
    Given the admin is logged in
    When the admin views their account status
    Then the remaining recovery code count is 10

  Scenario: Code count decreases after using a code
    Given the admin is logged in
    And one recovery code has been used for password reset
    When the admin views their account status
    Then the remaining recovery code count is 9

  Scenario: Code count resets to 10 after regeneration
    Given the admin is logged in
    And the admin has used 3 recovery codes
    And the admin has regenerated recovery codes
    When the admin views their account status
    Then the remaining recovery code count is 10
