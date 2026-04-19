Feature: Session Invalidation on Password Change

  When a user's password changes, all existing sessions for that user
  become invalid. This prevents a compromised session from persisting
  after a password reset.

  Scenario: Password change invalidates existing sessions
    Given an admin user is logged in on two devices
    When the user's password is changed
    Then both existing sessions are no longer valid
    And subsequent requests with the old sessions return 401

  Scenario: New login works after password change
    Given an admin user has changed their password
    When the user logs in with the new password
    Then a new session is created
    And the user can access protected routes
