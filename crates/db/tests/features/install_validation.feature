Feature: Installation validation

  The demo database ships pre-seeded with an admin account. Before the
  server opens to requests, it checks whether that account is present
  and has a usable password — a broken sidecar copy leaves the system
  in a state where no one can log in.

  Scenario: Properly seeded demo database passes validation
    Given a demo database with the admin account seeded and password set
    When the installation is validated against that database
    Then the validation passes

  Scenario: Demo database missing the admin account fails validation
    Given a demo database with no admin account
    When the installation is validated against that database
    Then the validation fails

  Scenario: Admin account without a password hash fails validation
    Given a demo database with an admin account but no password hash stored
    When the installation is validated against that database
    Then the validation fails

  Scenario: Admin account with an empty password hash fails validation
    Given a demo database with an admin account and an empty password hash
    When the installation is validated against that database
    Then the validation fails
