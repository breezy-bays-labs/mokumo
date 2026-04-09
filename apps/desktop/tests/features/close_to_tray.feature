@wip @desktop-only
Feature: Close to tray

  Closing the Mokumo window minimizes it to the system tray instead
  of killing the server. The server keeps running for LAN employees.
  Quitting (Cmd+Q, Alt+F4, tray menu) triggers graceful shutdown.

  # Note: these scenarios document desktop-specific behavior.
  # Automation requires Tauri test harness (deferred to build phase).

  # Window close → hide to tray

  Scenario: Closing the window hides to tray
    Given the Mokumo desktop app is running
    When I close the window with the X button
    Then the window is hidden
    And the server continues running
    And the tray icon is visible

  Scenario: macOS dock icon hides when window is minimized to tray
    Given the Mokumo desktop app is running on macOS
    When I close the window with the X button
    Then the dock icon is hidden
    And the tray icon is visible

  # Tray icon and menu

  Scenario: Tray icon shows server status
    Given the server is running with mDNS active
    Then the tray icon shows a green status dot

  Scenario: Tray icon shows yellow when mDNS is down
    Given the server is running but mDNS registration failed
    Then the tray icon shows a yellow status dot

  Scenario: Tray menu shows connection info
    Given the server is running on port 6565
    When I open the tray menu
    Then I see the mDNS address
    And I see the IP address
    And I see the port number

  Scenario: Reopening the desktop app from tray
    Given the window is hidden (minimized to tray)
    When I click "Reopen Desktop App" in the tray menu
    Then the window is shown and focused

  Scenario: Opening browser from tray
    Given the server is running
    When I click "Open in Browser" in the tray menu
    Then the default browser opens to the server URL

  Scenario: Left-clicking tray icon reopens window
    Given the window is hidden (minimized to tray)
    When I left-click the tray icon
    Then the window is shown and focused

  # Quit flow

  Scenario: Quitting from tray menu triggers shutdown
    Given the server is running
    When I click "Quit Mokumo" in the tray menu
    Then a shutdown dialog appears
    And the server begins graceful shutdown

  Scenario: Cmd+Q triggers quit with confirmation
    Given the Mokumo desktop app is running
    When I press Cmd+Q
    Then a confirmation dialog asks "Do you want to shut down Mokumo?"

  Scenario: Shutdown dialog shows progress
    Given the server is shutting down
    Then I see "Mokumo is shutting down..."
    And the dialog indicates the timeout period
    And the dialog closes when shutdown completes

  Scenario: Shutdown completes within 10 seconds
    Given the server is shutting down with in-flight requests
    Then the app exits within 10 seconds regardless

  # Quit confirmation cancel path

  Scenario: Cancelling quit returns to normal operation
    Given the Mokumo desktop app is running
    When I press Cmd+Q
    And the confirmation dialog appears
    And I click "No"
    Then the dialog closes
    And the server continues running
    And the window remains visible

  # Hidden-window shutdown notification

  Scenario: System notification sent when quitting from tray
    Given the window is hidden (minimized to tray)
    When I click "Quit Mokumo" in the tray menu
    Then a system notification is sent (best effort)
    And the server begins graceful shutdown

  # Port fallback in tray menu

  Scenario: Tray menu highlights fallback port
    Given the server started on fallback port 6567
    When I open the tray menu
    Then the port display indicates a non-default port

  # Desktop port exhaustion (C6)

  Scenario: Desktop shows error dialog when all ports exhausted
    Given ports 6565 through 6575 are already in use
    When the Mokumo desktop app starts
    Then an error dialog shows "All ports 6565-6575 are occupied"
    And the dialog suggests closing conflicting applications
    And the app exits after the dialog is dismissed

  # Linux tray degradation

  Scenario: Close behaves as quit when tray is unavailable
    Given the system does not support tray icons
    When I close the window with the X button
    Then the quit confirmation dialog appears
    And the server does not hide to tray
