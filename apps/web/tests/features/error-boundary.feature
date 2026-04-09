Feature: Error boundary

  The branded error page (+error.svelte) renders for invalid routes and API errors.
  Displays the Mokumo logo, status code, human-readable title, and navigation options.

  Scenario: Unknown route shows branded 404 error page
    When I navigate to "/this-route-does-not-exist"
    Then I see the branded error page
    And the error page shows status "404"
    And the error page shows title "Page not found"
    And the error page has a "Return to Dashboard" link

  Scenario: Error page has a go-back button
    When I navigate to "/this-route-does-not-exist"
    Then I see the branded error page
    And the error page has a "Go back" button
