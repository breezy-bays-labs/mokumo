@wip
Feature: API pagination

  When listing resources, the API returns paginated results
  with metadata so the frontend can build pagination controls.

  Background:
    Given the API server is running

  # --- Default pagination ---

  Scenario: Listing resources without pagination params uses defaults
    Given 50 customers exist
    When I list customers without specifying pagination
    Then the response should contain 25 items
    And the page should be 1
    And the per_page should be 25
    And the total should be 50
    And the total_pages should be 2

  # --- Custom pagination ---

  Scenario: Requesting a specific page and page size
    Given 50 customers exist
    When I list customers on page 2 with 10 per page
    Then the response should contain 10 items
    And the page should be 2
    And the per_page should be 10
    And the total should be 50
    And the total_pages should be 5

  # --- Clamping ---

  Scenario: Page size above maximum is clamped to 100
    Given 200 customers exist
    When I list customers with 500 per page
    Then the per_page should be 100
    And the response should contain 100 items

  Scenario: Page below 1 is clamped to 1
    Given 10 customers exist
    When I list customers on page 0
    Then the page should be 1

  Scenario: Page size below 1 uses the default
    Given 30 customers exist
    When I list customers with 0 per page
    Then the per_page should be 25

  # --- Empty collection ---

  Scenario: Empty collection returns zero total pages
    Given no customers exist
    When I list customers
    Then the response should contain 0 items
    And the total should be 0
    And the total_pages should be 0
    And the page should be 1

  # --- Out of range ---

  Scenario: Requesting a page beyond the last returns empty items
    Given 10 customers exist
    When I list customers on page 99
    Then the response should contain 0 items
    And the total should be 10
    And the total_pages should be 1

  # --- Paginated response shape ---

  Scenario: Paginated responses always include metadata
    Given 5 customers exist
    When I list customers
    Then the response should include "items", "total", "page", "per_page", and "total_pages"
