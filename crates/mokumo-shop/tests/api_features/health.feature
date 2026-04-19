Feature: Health endpoint

  Scenario: Health check returns OK
    Given the API server is running
    When I request GET "/api/health"
    Then the response status should be 200
