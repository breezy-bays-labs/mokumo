@wip
Feature: User management — last-admin guard

  Admin accounts can be deactivated or demoted to a lower role. The shop
  must always retain at least one active admin. Any operation that would
  leave zero active admins is rejected. Deactivated admins do not count
  as active and cannot satisfy this requirement.

  Background:
    Given the API server is running

  # --- Deactivate (soft-delete) happy paths ---

  Scenario: Deactivating a non-admin user succeeds
    Given an admin user exists
    And a staff user exists
    And the admin user is logged in
    When the admin deactivates the staff user
    Then the staff user is deactivated
    And the deactivation is recorded in the activity log

  Scenario: Deactivating one of two admins succeeds
    Given two admin users exist
    And one of the admins is logged in
    When that admin deactivates the other admin
    Then the other admin is deactivated
    And one active admin remains

  # --- Deactivate last-admin guard ---

  Scenario: Deactivating the last active admin is rejected
    Given a single active admin exists
    And that admin is logged in
    When the admin attempts to deactivate their own account
    Then the operation is rejected
    And the rejection message is "Cannot delete the last admin account. Assign another admin first."
    And no activity log entry is created for this operation

  # --- Role update happy paths ---

  Scenario: Promoting a staff user to admin succeeds
    Given an admin user exists
    And a staff user exists
    And the admin user is logged in
    When the admin promotes the staff user to admin
    Then the staff user now has the admin role
    And the role change is recorded in the activity log

  Scenario: Demoting one of two admins succeeds
    Given two admin users exist
    And one of the admins is logged in
    When that admin demotes the other admin to staff
    Then the other user now has the staff role
    And one active admin remains

  # --- Role update last-admin guard ---

  Scenario: Demoting the last active admin is rejected
    Given a single active admin exists
    And that admin is logged in
    When the admin attempts to demote themselves to staff
    Then the operation is rejected
    And the rejection message is "Cannot demote the last admin account. Assign another admin first."
    And no activity log entry is created for this operation

  # --- Boundary: deactivated admin does not count ---

  Scenario: A deactivated admin does not count when demoting the last active admin
    Given a single active admin exists
    And a previously-deactivated admin account exists
    And the active admin is logged in
    When the active admin attempts to demote themselves to staff
    Then the operation is rejected
    And the rejection message is "Cannot demote the last admin account. Assign another admin first."

  Scenario: A deactivated admin does not count when deactivating the last active admin
    Given a single active admin exists
    And a previously-deactivated admin account exists
    And the active admin is logged in
    When the active admin attempts to deactivate their own account
    Then the operation is rejected
    And the rejection message is "Cannot delete the last admin account. Assign another admin first."

  # --- Boundary: promote-then-demote unlock ---

  Scenario: Promoting a second admin then demoting the first is allowed
    Given a single active admin exists
    And a staff user exists
    And the admin is logged in
    When the admin promotes the staff user to admin
    And the admin demotes themselves to staff
    Then the originally-promoted user is now the sole active admin

  # --- Authorization ---

  Scenario: A non-admin caller cannot deactivate users
    Given an admin user exists
    And a staff user exists
    And the staff user is logged in
    When the staff user attempts to deactivate the admin
    Then the response status is 403
    And the error code is "forbidden"

  Scenario: A non-admin caller cannot change user roles
    Given an admin user exists
    And a staff user exists
    And the staff user is logged in
    When the staff user attempts to change the admin's role
    Then the response status is 403
    And the error code is "forbidden"
