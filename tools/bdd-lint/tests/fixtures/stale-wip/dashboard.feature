Feature: Dashboard

  @wip
  Scenario: User views dashboard stats
    Given the user is logged in
    When the user navigates to the dashboard
    Then the dashboard shows total orders

  @wip
  Scenario: User exports dashboard report
    Given the user is logged in
    When the user clicks export to PDF
    Then a PDF report is downloaded

  Scenario: User sees welcome message
    Given the user is logged in
    When the user navigates to the dashboard
    Then a welcome message is displayed
