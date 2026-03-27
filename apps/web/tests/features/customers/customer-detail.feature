Feature: Customer detail page and tab navigation

  The detail page shows a single customer's complete information organized
  across tabbed sections. The header and tab navigation persist while tab
  content changes with the URL. Each tab is a real route — bookmarkable,
  shareable, and accessible via direct navigation.

  # --- Header (V4) ---

  Scenario: Detail page shows the customer name and company
    Given a customer "Acme Printing" with company "Acme Corp" exists
    When I navigate to the Acme Printing detail page
    Then the header shows "Acme Printing"
    And the header shows "Acme Corp"

  Scenario: Archived customer shows a deleted badge
    Given an archived customer "Old Corp" exists
    When I navigate to the Old Corp detail page
    Then I see a deleted badge on the header

  # --- Overview Tab (V4) ---

  Scenario: Overview tab shows contact information
    Given a customer with email "acme@example.com" and phone "555-1234"
    When I view the customer's overview tab
    Then the overview shows email "acme@example.com"
    And the overview shows phone "555-1234"

  Scenario: Overview tab shows the mailing address
    Given a customer with address "123 Main St, Springfield, IL 62701"
    When I view the customer's overview tab
    Then the overview shows the full mailing address

  Scenario: Overview tab shows notes
    Given a customer with notes "Prefers rush delivery"
    When I view the customer's overview tab
    Then the overview shows notes "Prefers rush delivery"

  Scenario: Overview tab shows financial defaults
    Given a customer with payment terms "Net 30" and credit limit "$5,000"
    When I view the customer's overview tab
    Then the overview shows payment terms "Net 30"
    And the overview shows credit limit "$5,000"

  Scenario: Overview tab shows tax-exempt status
    Given a customer who is tax exempt
    When I view the customer's overview tab
    Then the overview indicates the customer is tax exempt

  # --- Tab Navigation (V5) ---

  Scenario: Detail page shows six navigation tabs
    Given I am on a customer's detail page
    Then I see tabs for Overview, Activity, Contacts, Artwork, Pricing, and Communication

  Scenario Outline: Clicking a tab navigates to its route
    Given I am on customer "Acme Printing"'s detail page
    When I click the "<tab>" tab
    Then the URL path includes "<segment>"
    And the "<tab>" tab is active

    Examples:
      | tab           | segment       |
      | Activity      | /activity     |
      | Contacts      | /contacts     |
      | Artwork       | /artwork      |
      | Pricing       | /pricing      |
      | Communication | /communication|

  Scenario: Overview is the default tab
    When I navigate to a customer's detail page
    Then the Overview tab is active
    And no tab segment appears in the URL

  Scenario: Tab routes are directly accessible via URL
    When I navigate directly to a customer's activity tab URL
    Then the Activity tab content is displayed
    And the Activity tab is active
    And the customer header is visible
