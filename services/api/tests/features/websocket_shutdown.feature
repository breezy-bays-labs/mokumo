Feature: WebSocket graceful shutdown

  Scenario: Server sends shutdown notification then close frame
    Given the API server is running
    And a client is connected to "/ws"
    When the server begins shutting down
    Then the client receives a message with type "server_shutting_down"
    And the client receives a close frame with code 1001
