Feature: Offline startup validation

  Mokumo's core local workflow must function with zero internet access.
  Optional online dependencies (mDNS LAN discovery) must degrade
  gracefully without blocking boot or the internal shop API.

  Background:
    Given the server is started with "--host 0.0.0.0"
    And mDNS registration will fail

  @offline
  Scenario: Health endpoint responds when mDNS registration fails
    When the server starts
    Then the server is running
    And the health response includes database ok

  @offline
  Scenario: Server-info reports degraded LAN state when offline
    When the server starts
    And a client requests the server info endpoint
    Then the response shows LAN access is disabled
    And the LAN URL is absent

  @offline
  Scenario: Authenticated shop API responds when mDNS is unavailable
    Given setup is completed
    When the server starts
    And the diagnostics endpoint is requested
    Then the diagnostics return 200
    And the diagnostics show mdns_active is false
