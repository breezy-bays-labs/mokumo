Feature: Customer create and edit forms

  A slide-in sheet for creating new customers and editing existing ones.
  The same form handles both modes: empty for create, pre-populated for edit.
  Validation runs client-side for immediate feedback; the server is authoritative
  and its errors are mapped back to the relevant form fields.

  # --- Create (V3) ---

  Scenario: Creating a customer with required fields
    Given I am on the Customers page
    And I open the create customer form
    When I fill in "Display name" with "Acme Printing"
    And I submit the form
    Then I see a "Customer created" toast notification
    And the form sheet closes
    And "Acme Printing" appears in the customer list

  Scenario: Creating a customer with all fields
    Given I open the create customer form
    When I fill in the following fields:
      | field           | value               |
      | Display name    | Acme Printing       |
      | Company name    | Acme Corp           |
      | Email           | info@acme.com       |
      | Phone           | 555-1234            |
      | Address line 1  | 123 Main St         |
      | City            | Springfield         |
      | State           | IL                  |
      | Postal code     | 62701               |
      | Notes           | Prefers rush orders |
      | Payment terms   | Net 30              |
      | Credit limit    | 5000                |
    And I submit the form
    Then I see a "Customer created" toast notification

  Scenario: Display name is required
    Given I open the create customer form
    When I leave "Display name" empty
    And I submit the form
    Then I see a validation error on "Display name"
    And the form sheet remains open

  Scenario: Invalid email shows a validation error
    Given I open the create customer form
    When I fill in "Display name" with "Test Customer"
    And I fill in "Email" with "not-an-email"
    And I submit the form
    Then I see a validation error on "Email"

  Scenario: Server validation errors appear on the correct fields
    Given I open the create customer form
    When the server rejects my submission with a field error on "email"
    Then I see the server's error message on the "Email" field

  Scenario: Cancelling the form discards changes
    Given I open the create customer form
    And I fill in "Display name" with "Draft Customer"
    When I close the form sheet
    Then "Draft Customer" does not appear in the customer list

  Scenario: Creating a customer with a duplicate name succeeds
    Given a customer "Acme Printing" already exists
    When I create another customer named "Acme Printing"
    Then I see a "Customer created" toast notification
    And both customers appear in the list

  # --- Edit (V6) ---

  Scenario: Edit button opens a pre-populated form
    Given I am on the detail page for customer "Acme Printing"
    When I click "Edit"
    Then the customer form sheet opens
    And the "Display name" field shows "Acme Printing"

  Scenario: Editing a field updates the customer
    Given I am editing customer "Acme Printing"
    When I change "Phone" to "555-9876"
    And I submit the form
    Then I see a "Customer updated" toast notification
    And the detail page shows "555-9876" as the phone number

  Scenario: Clearing an optional field removes the value
    Given customer "Acme Printing" has email "acme@example.com"
    And I am editing customer "Acme Printing"
    When I clear the "Email" field
    And I submit the form
    Then the detail page no longer shows an email address

  Scenario: Edit form reflects the latest customer data
    Given customer "Acme Printing" has company "Acme Corp", email "info@acme.com", and phone "555-1234"
    And I am on the detail page for customer "Acme Printing"
    When I click "Edit"
    Then the form shows the following values:
      | field        | value          |
      | Display name | Acme Printing  |
      | Company name | Acme Corp      |
      | Email        | info@acme.com  |
      | Phone        | 555-1234       |
