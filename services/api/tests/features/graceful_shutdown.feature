@wip
Feature: Graceful shutdown

  Mokumo drains in-flight requests and notifies connected clients
  before exiting. Shutdown completes within 10 seconds regardless.

  # Signal handling

  Scenario: Server shuts down on Ctrl+C
    Given the server is running
    When the server receives SIGINT
    Then the server begins graceful shutdown
    And in-flight requests are allowed to complete

  @unix
  Scenario: Server shuts down on SIGTERM
    Given the server is running
    When the server receives SIGTERM
    Then the server begins graceful shutdown
    And in-flight requests are allowed to complete

  # Drain timeout

  Scenario: Server exits within 10 seconds even with slow requests
    Given the server is running
    And a request is in-flight that will take 30 seconds
    When the server begins shutting down
    Then the server exits within 10 seconds

  Scenario: Server exits immediately when no requests are in-flight
    Given the server is running
    And no requests are in-flight
    When the server begins shutting down
    Then the server exits without waiting for the timeout

  # WebSocket shutdown notification

  Scenario: Connected clients receive shutdown message before drain
    Given the server is running
    And a client is connected to "/ws"
    When the server begins shutting down
    Then the client receives a message with type "server_shutting_down"
    And then the client receives a close frame with code 1001

  Scenario: Multiple clients all receive shutdown message
    Given the server is running
    And 3 clients are connected to "/ws"
    When the server begins shutting down
    Then all 3 clients receive a message with type "server_shutting_down"

  # mDNS cleanup on CLI restart

  Scenario: mDNS is deregistered before re-registration on restart
    Given the CLI server is running with mDNS registered
    When the server restarts via the restart sentinel
    Then mDNS is deregistered before the new server initializes
    And mDNS is re-registered with the new server port

  # Resource cleanup

  Scenario: Background tasks stop on shutdown
    Given the server is running
    And background tasks are active (IP refresh, session cleanup, PIN sweep)
    When the server begins shutting down
    Then all background tasks are cancelled
    And no background tasks are running after shutdown completes
