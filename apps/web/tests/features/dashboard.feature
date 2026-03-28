Feature: Dashboard displays LAN connection information

  The dashboard is the admin's home base after login. It shows server
  status, the correct LAN URL for sharing with employees, and a
  "Connect Your Team" card that explains how other devices can access
  Mokumo on the local network.

  # --- LAN URL Display ---

  Scenario: Dashboard shows the LAN URL from server info
    Given the server has mDNS active with hostname "mokumo.local" on port 3000
    When I navigate to the dashboard
    Then I see the LAN URL "http://mokumo.local:3000"

  Scenario: Dashboard falls back to IP address when mDNS is inactive
    Given the server has mDNS inactive with IP "192.168.1.42" on port 3000
    When I navigate to the dashboard
    Then I see the LAN URL "http://192.168.1.42:3000"

  Scenario: LAN URL can be copied to clipboard
    Given the server has mDNS active with hostname "mokumo.local" on port 3000
    And I am on the dashboard
    When I copy the LAN URL
    Then the clipboard contains "http://mokumo.local:3000"
    And I see a "URL copied to clipboard" toast message

  # --- Connect Your Team Card ---

  Scenario: Connect Your Team card appears when LAN URL is available
    Given the server has mDNS active with hostname "mokumo.local" on port 3000
    When I navigate to the dashboard
    Then I see the "Connect Your Team" card
    And I see "Share this with your team"

  Scenario: Connect Your Team card is hidden when no LAN access exists
    Given the server has no LAN access
    When I navigate to the dashboard
    Then I do not see the "Connect Your Team" card

  # --- Edge Cases ---

  @future
  Scenario: Dashboard handles server-info fetch failure gracefully
    Given the server-info API is unavailable
    When I navigate to the dashboard
    Then the LAN URL shows "—"
    And I do not see the "Connect Your Team" card
    And the server status still displays

  # --- Server Status ---

  Scenario: Dashboard shows server status as online
    Given the server is healthy
    When I navigate to the dashboard
    Then I see the server status as "Online"
