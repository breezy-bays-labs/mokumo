Feature: Login flows

  @wip
  Scenario: Future login flow
    Given a user with SSO enabled
    When the user authenticates via SSO
    Then the user is redirected to the dashboard

  Scenario: Current login flow
    Given a user with email "user@test.com"
    When the user enters their password
    Then the user is logged in
