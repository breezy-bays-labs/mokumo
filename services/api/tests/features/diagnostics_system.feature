Feature: Diagnostics system health surface

  The diagnostics endpoint exposes system-level health signals so that support
  can perform first-pass triage without SSH access.

  Background:
    Given the API server is running
    And an admin user is logged in

  Scenario: Diagnostics response includes the system object
    When I request GET "/api/diagnostics"
    Then the response status should be 200
    And the response should include "system"

  Scenario: System memory values are non-negative integers
    When I request GET "/api/diagnostics"
    Then the json path "system.total_memory_bytes" should be a non-negative integer
    And the json path "system.used_memory_bytes" should be a non-negative integer

  Scenario: System disk values are non-negative integers
    When I request GET "/api/diagnostics"
    Then the json path "system.disk_total_bytes" should be a non-negative integer
    And the json path "system.disk_free_bytes" should be a non-negative integer

  Scenario: Diagnostics includes app build commit
    When I request GET "/api/diagnostics"
    Then the json path "app.build_commit" should exist
