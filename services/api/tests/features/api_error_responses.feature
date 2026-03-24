@wip
Feature: API error responses

  When something goes wrong processing a request,
  the API returns a consistent error shape that the
  frontend can parse without guessing.

  Background:
    Given the API server is running

  Scenario: Looking up a resource that does not exist
    When I request a customer that does not exist
    Then the response status should be 404
    And the error code should be "not_found"
    And the error message should contain "customer"

  Scenario: Submitting invalid data shows errors grouped by field
    When I create a customer with an invalid email and no name
    Then the response status should be 422
    And the error code should be "validation_error"
    And the error details should be keyed by field name
    And the error details should include "email" with at least one message
    And the error details should include "name" with at least one message

  Scenario: Creating a duplicate resource
    Given a customer with email "gary@4ink.com" exists
    When I create another customer with email "gary@4ink.com"
    Then the response status should be 409
    And the error code should be "conflict"

  Scenario: Internal errors do not leak details to the client
    When an unexpected internal error occurs
    Then the response status should be 500
    And the error code should be "internal_error"
    And the error message should not contain stack trace details
    And the error details should not be present
