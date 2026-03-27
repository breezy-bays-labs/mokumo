Feature: Customer list page

  The customer list is the primary entry point for managing customers.
  It displays all active customers in a table with search, filtering,
  pagination, and a toggle to reveal archived customers. All filter
  state lives in the URL so it survives refresh and back-button navigation.

  # --- Empty State ---

  Scenario: Empty customer list shows a call to action
    Given no customers exist in the system
    When I navigate to the Customers page
    Then I see an empty state with an "Add Customer" prompt

  # --- Error State ---

  Scenario: Error state shows when the API is unreachable
    Given the API is unavailable
    When I navigate to the Customers page
    Then I see an error message indicating the data could not be loaded

  # --- Data Display (V1) ---

  Scenario: Customer list loads with a data table
    Given customers exist in the system
    When I navigate to the Customers page
    Then I see a table of customers
    And each row shows the customer's name, company, email, and phone

  Scenario: Loading skeleton appears while data is fetching
    When I navigate to the Customers page
    Then I see a loading skeleton before the table appears

  Scenario: Total customer count is displayed above the table
    Given 15 customers exist in the system
    When I navigate to the Customers page
    Then the KPI strip shows "15" as the total customer count

  Scenario: Clicking a customer row opens the detail page
    Given a customer "Acme Printing" exists
    When I click the "Acme Printing" row in the table
    Then I am on the Acme Printing detail page

  Scenario: Add Customer button opens the create form
    Given I am on the Customers page
    When I click "Add Customer"
    Then the customer form sheet opens
    And the form fields are empty

  # --- Search (V2 — requires P0 backend search param) ---

  Scenario: Searching filters the customer list
    Given customers "Acme Printing" and "Beta Apparel" exist
    When I type "acme" in the search bar
    Then only "Acme Printing" appears in the table

  Scenario: Clearing the search shows all customers
    Given I am searching for "acme" on the Customers page
    When I clear the search bar
    Then all customers appear in the table

  # --- Soft Delete Toggle (V2) ---

  Scenario: Archived customers are hidden by default
    Given an archived customer "Old Corp" exists
    When I navigate to the Customers page
    Then "Old Corp" does not appear in the table

  Scenario: Show-deleted toggle reveals archived customers
    Given an archived customer "Old Corp" exists
    And I am on the Customers page
    When I toggle "Show deleted"
    Then "Old Corp" appears in the table

  # --- Pagination (V2) ---

  Scenario: Pagination controls appear when results span multiple pages
    Given more customers exist than fit on one page
    When I navigate to the Customers page
    Then I see pagination controls

  Scenario: Clicking next page shows the next set of customers
    Given I am on page 1 of the customer list
    When I click the next page button
    Then I see a different set of customers
    And the URL reflects page 2

  # --- URL State Persistence (V2) ---

  Scenario: Search state survives page refresh
    Given I am searching for "acme" on the Customers page
    When I refresh the page
    Then the search bar still shows "acme"
    And the table is filtered to match "acme"

  Scenario: Filter state works with browser back button
    Given I am on the Customers page with no filters
    And I search for "acme"
    When I press the browser back button
    Then the search bar is empty
    And all customers appear in the table

  Scenario: Pagination state survives page refresh
    Given I am on page 2 of the customer list
    When I refresh the page
    Then I am still on page 2

  # --- Responsive (R5) ---

  Scenario: Customer list adapts to narrow viewports
    Given customers exist in the system
    When I view the Customers page on a mobile viewport
    Then customer information is still readable and accessible
