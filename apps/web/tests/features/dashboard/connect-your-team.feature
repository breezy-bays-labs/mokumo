@wip
Feature: Connect Your Team card on dashboard

  The admin dashboard shows a "Connect Your Team" card with a QR code
  and connection URL so shop owners can onboard employees to the LAN.
  The same card component appears on the Settings page (tested via
  settings-lan-status.feature, which will be extended during build).

  Background:
    Given the server-info API returns LAN status

  Scenario: Connect Your Team card is visible on the dashboard
    When I navigate to the dashboard
    Then I see the "Connect Your Team" card

  Scenario: QR code encodes the IP-based URL
    Given the server is running on 192.168.1.50 port 6565
    When I navigate to the dashboard
    Then I see a QR code
    And the QR code encodes "http://192.168.1.50:6565"

  Scenario: QR code uses IP even when mDNS is active
    Given mDNS is active as "mokumo.local"
    And the server IP is 192.168.1.50 on port 6565
    When I navigate to the dashboard
    Then the QR code encodes "http://192.168.1.50:6565"

  Scenario: Copy connection link copies IP-based URL
    When I navigate to the dashboard
    And I click "Copy connection link"
    Then the clipboard contains the IP-based URL
    And I see a "Link copied" toast message

  Scenario: mDNS status shows active
    Given mDNS is active
    When I navigate to the dashboard
    Then I see a green status dot
    And the status text reads "LAN discovery active"

  Scenario: mDNS status shows unavailable
    Given mDNS is inactive
    When I navigate to the dashboard
    Then I see a yellow status dot
    And the status text reads "Unavailable — use IP address"

  Scenario: mDNS URL displayed when active
    Given mDNS is active as "mokumo.local" on port 6565
    When I navigate to the dashboard
    Then I see the mDNS URL "http://mokumo.local:6565"

  Scenario: IP URL always displayed
    When I navigate to the dashboard
    Then I see the IP address URL

  Scenario: Troubleshooting guidance shown when mDNS unavailable
    Given mDNS is inactive
    When I navigate to the dashboard
    Then I see troubleshooting text mentioning "AP isolation" and "multicast filtering"

  Scenario: First-run nudge highlights the card for new installs
    Given no employee sessions have ever been created
    When I navigate to the dashboard
    Then the "Connect Your Team" card has a visual highlight
    And I see a "New" badge on the card

  Scenario: First-run nudge disappears after first employee connects
    Given an employee has connected at least once
    When I navigate to the dashboard
    Then the "Connect Your Team" card has no visual highlight
    And I do not see a "New" badge
