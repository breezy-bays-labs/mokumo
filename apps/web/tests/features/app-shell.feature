@wip
Feature: App shell navigation and layout

  The app shell provides persistent sidebar navigation, a top bar with
  breadcrumbs, and route-level content for all sections of Mokumo.

  # --- Navigation ---

  Scenario: Sidebar displays seven navigation items
    Given the app shell is loaded
    Then the sidebar shows exactly 7 navigation items
    And the navigation items are in this order:
      | label      |
      | Home       |
      | Customers  |
      | Quotes     |
      | Orders     |
      | Invoices   |
      | Artwork    |
      | Settings   |

  Scenario: Every navigation item has an icon
    Given the app shell is loaded
    Then every sidebar navigation item displays an icon

  Scenario Outline: Clicking a navigation item navigates to its route
    Given the app shell is loaded
    When I click "<label>" in the sidebar
    Then the URL is "<route>"
    And "<label>" is the active sidebar item

    Examples:
      | label     | route      |
      | Home      | /          |
      | Customers | /customers |
      | Quotes    | /quotes    |
      | Orders    | /orders    |
      | Invoices  | /invoices  |
      | Artwork   | /artwork   |
      | Settings  | /settings/shop |

  Scenario: Hidden routes are not shown in the sidebar
    Given the app shell is loaded
    Then the sidebar does not show "Production"
    And the sidebar does not show "Shipping"
    And the sidebar does not show "Garments"

  Scenario Outline: Hidden routes are accessible via direct URL
    When I navigate directly to "<route>"
    Then the page renders an empty state
    And the empty state title is "<title>"

    Examples:
      | route       | title      |
      | /production | Production |
      | /shipping   | Shipping   |
      | /garments   | Garments   |

  # --- Top Bar ---

  Scenario: Top bar shows breadcrumbs matching the current route
    Given I am on the Quotes page
    Then the breadcrumb trail shows "Home / Quotes"

  Scenario: Top bar shows a sidebar toggle button
    Given the app shell is loaded
    Then the top bar contains a sidebar toggle button

  Scenario: Top bar shows a user avatar placeholder
    Given the app shell is loaded
    Then the top bar shows a generic user icon

  # --- Home Page ---

  Scenario: Home page displays server health when running
    Given the server is running
    When I am on the Home page
    Then I see a green health indicator
    And I see the app version

  Scenario: Home page displays server health when unreachable
    Given the server is not responding
    When I am on the Home page
    Then I see a red health indicator

  Scenario: Home page shows the shop name placeholder
    Given I am on the Home page
    Then I see "Your Shop" as the shop name
    And I see "Powered by Mokumo" branding

  Scenario: Home page shows the LAN access URL
    Given I am on the Home page
    Then I see a LAN access URL for reaching this server

  Scenario: Home page shows a getting-started card
    Given I am on the Home page
    Then I see a "Create your first customer" card
    And the card links to the Customers page

  # --- Empty States ---

  Scenario Outline: Unbuilt sections show an empty state
    When I navigate to the <section> page
    Then I see an empty state with an icon
    And the empty state title is "<title>"
    And the empty state subtitle mentions active development

    Examples:
      | section  | title    |
      | Quotes   | Quotes   |
      | Orders   | Orders   |
      | Invoices | Invoices |
      | Artwork  | Artwork  |

  Scenario: Customer page shows distinct empty state messaging
    When I navigate to the Customers page
    Then I see an empty state with an icon
    And the empty state title is "Customers"
    And the empty state subtitle is "This is where your customers will live. Coming in the next session."

  # --- Settings ---

  Scenario: Visiting settings redirects to the Shop tab
    When I navigate directly to "/settings"
    Then the URL changes to "/settings/shop"
    And the Shop tab is active

  Scenario Outline: Settings tabs navigate between sub-routes
    Given I am on the Settings page
    When I click the "<tab>" tab
    Then the URL is "/settings/<path>"
    And the "<tab>" tab is active
    And the tab content shows an empty state

    Examples:
      | tab     | path    |
      | Shop    | shop    |
      | Account | account |
      | System  | system  |

  # --- Sidebar Interactivity ---

  Scenario: Sidebar collapses to icon rail
    Given the sidebar is expanded
    When I click the sidebar rail handle
    Then the sidebar collapses to an icon rail
    And navigation items show only icons

  Scenario: Collapsed sidebar state persists across page refresh
    Given the sidebar is collapsed
    When I refresh the page
    Then the sidebar is still collapsed

  Scenario: Keyboard shortcut toggles the sidebar
    Given the sidebar is expanded
    When I press the sidebar toggle keyboard shortcut
    Then the sidebar collapses to an icon rail

  Scenario: Sidebar footer shows user avatar and name
    Given the sidebar is expanded
    Then the sidebar footer displays a user avatar and name

  Scenario: Sidebar footer popover offers theme selection
    Given I click the sidebar footer avatar
    Then a popover appears with a theme selector

  Scenario: Logout returns to the Home page
    Given the sidebar footer popover is open on the Customers page
    When I click Logout
    Then I am on the Home page

  # --- Mobile ---

  Scenario: Sidebar becomes a sheet overlay on mobile
    Given the viewport is narrower than 768 pixels
    Then the sidebar is not visible
    And a menu trigger button is shown in the top bar

  Scenario: Mobile sheet opens when trigger is clicked
    Given the viewport is narrower than 768 pixels
    When I click the menu trigger button
    Then a sidebar sheet slides in from the left
    And I cannot interact with the page behind the sheet

  Scenario: Mobile sheet closes after navigation
    Given the mobile sidebar sheet is open
    When I click "Quotes" in the sheet
    Then the sheet closes
    And I am on the Quotes page

  Scenario: Mobile sheet closes when backdrop is clicked
    Given the mobile sidebar sheet is open
    When I click the backdrop overlay
    Then the sheet closes
