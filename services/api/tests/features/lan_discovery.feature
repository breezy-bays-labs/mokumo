Feature: LAN Discovery

  Mokumo registers itself on the local network so shop employees
  can access the app by typing "mokumo.local" instead of an IP address.
  Discovery only activates when the server is bound to all interfaces.

  # V1 + V3: Registration and guard

  Scenario: Server registers on the local network when bound to all interfaces
    Given the server is started with "--host 0.0.0.0"
    When the server starts
    Then mDNS is registered as "mokumo.local" on the actual bound port
    And the service type is "_http._tcp"

  Scenario: Server skips LAN discovery when bound to localhost
    Given no CLI flags are provided
    When the server starts
    Then mDNS is not registered
    And the log contains "mDNS registration skipped"

  Scenario: Server starts even when LAN discovery fails
    Given the server is started with "--host 0.0.0.0"
    And mDNS registration will fail
    When the server starts
    Then the server is running
    And the log contains "mDNS registration failed"

  # V1 + port fallback interaction

  Scenario: LAN discovery uses the actual bound port after fallback
    Given the server is started with "--host 0.0.0.0"
    And port 6565 is already in use
    When the server starts
    Then mDNS is registered on the actual bound port

  # V2: Shutdown

  Scenario: Server deregisters from the network on shutdown
    Given the server is started with "--host 0.0.0.0"
    And mDNS is registered
    When the server begins shutting down
    Then the mDNS service is deregistered

  # V4: Server info endpoint

  Scenario: Server info reports active LAN access
    Given the server is started with "--host 0.0.0.0"
    And mDNS is registered as "mokumo.local"
    When a client requests the server info endpoint
    Then the response shows LAN access is active
    And the LAN URL is "http://mokumo.local" with the server port
    And an IP-based URL is included as fallback

  Scenario: Server info reports LAN access disabled on localhost
    Given the server is started with default host
    When a client requests the server info endpoint
    Then the response shows LAN access is disabled
    And the LAN URL is absent
    And no IP-based URL is included

  Scenario: Server info reports LAN access unavailable when mDNS fails
    Given the server is started with "--host 0.0.0.0"
    And mDNS registration has failed
    When a client requests the server info endpoint
    Then the response shows mDNS is not active
    And the LAN URL is absent
    And an IP-based URL is included as fallback

  # V5: Collision handling

  Scenario: Name collision updates the registered hostname
    Given the server is started with "--host 0.0.0.0"
    And mDNS is registered as "mokumo.local"
    When another device registers the same hostname
    Then the registered hostname is no longer "mokumo.local"
    And the log contains "mDNS name collision"

  @wip
  # V6: Settings page (user-facing outcome)

  Scenario: Shop settings page shows LAN connection details
    Given the server is started with "--host 0.0.0.0"
    And mDNS is registered as "mokumo.local"
    When an employee opens the shop settings page
    Then the LAN access status shows "active"
    And the LAN URL is displayed with a copy button
    And an IP fallback URL is displayed
