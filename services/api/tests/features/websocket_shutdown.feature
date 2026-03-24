Feature: WebSocket graceful shutdown

  Scenario: Server sends close frame before shutting down
    Given the API server is running
    And a client is connected to "/ws"
    When the server begins shutting down
    Then the client receives a close frame with code 1001
