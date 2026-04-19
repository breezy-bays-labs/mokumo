@wip
Feature: Host-header allowlist (DNS-rebinding defense)

  The server rejects any HTTP request whose Host header is not
  in the loopback allowlist. This prevents DNS-rebinding attacks
  where a malicious webpage targets 127.0.0.1:{port}.

  Background:
    Given the API server is running with the host-header allowlist

  # --- Rejection scenarios (R0, R2, R7) ---

  Scenario: Request with evil Host header is rejected
    When I send a request to "/api/health" with Host "evil.com"
    Then the response status should be 403
    And the error code should be "HOST_NOT_ALLOWED"
    And the response should have header "content-type" containing "application/json"
    And the response should have header "cache-control" equal to "no-store"

  Scenario: Missing Host header is rejected
    When I send a request to "/api/health" with no Host header
    Then the response status should be 403
    And the error code should be "HOST_NOT_ALLOWED"

  Scenario: Multiple Host headers are rejected
    When I send a request to "/api/health" with Host headers "127.0.0.1" and "evil.com"
    Then the response status should be 403
    And the error code should be "HOST_NOT_ALLOWED"

  Scenario: Subdomain spoofing with loopback prefix is rejected
    When I send a request to "/api/health" with Host "127.0.0.1.evil.com"
    Then the response status should be 403
    And the error code should be "HOST_NOT_ALLOWED"

  Scenario: Subdomain spoofing with localhost prefix is rejected
    When I send a request to "/api/health" with Host "localhost.evil.com"
    Then the response status should be 403
    And the error code should be "HOST_NOT_ALLOWED"

  Scenario: Percent-encoded Host header is rejected
    When I send a request to "/api/health" with Host "%6c%6f%63%61%6c%68%6f%73%74"
    Then the response status should be 403
    And the error code should be "HOST_NOT_ALLOWED"

  # --- Acceptance scenarios (R1) ---

  Scenario: Loopback 127.0.0.1 with port is accepted
    When I send a request to "/api/health" with Host "127.0.0.1:6565"
    Then the response status should be 200

  Scenario: Loopback 127.0.0.1 without port is accepted
    When I send a request to "/api/health" with Host "127.0.0.1"
    Then the response status should be 200

  Scenario: Localhost is accepted (case-insensitive)
    When I send a request to "/api/health" with Host "LOCALHOST"
    Then the response status should be 200

  Scenario: Lowercase localhost is accepted
    When I send a request to "/api/health" with Host "localhost"
    Then the response status should be 200

  Scenario: IPv6 loopback is accepted
    When I send a request to "/api/health" with Host "[::1]"
    Then the response status should be 200

  Scenario: IPv6 loopback with port is accepted
    When I send a request to "/api/health" with Host "[::1]:6565"
    Then the response status should be 200

  # --- WebSocket upgrade path (R7) ---

  Scenario: WebSocket upgrade with bad Host is rejected before reaching handler
    When I send a WebSocket upgrade to "/ws" with Host "evil.com"
    Then the response status should be 403
    And the error code should be "HOST_NOT_ALLOWED"
