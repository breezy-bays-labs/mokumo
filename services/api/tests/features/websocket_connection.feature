Feature: WebSocket connection

  Scenario: Client connects to the WebSocket endpoint
    Given the API server is running
    When a client connects to "/ws"
    Then the connection is accepted
    And the server tracks 1 connected client

  Scenario: Multiple clients connect simultaneously
    Given the API server is running
    When 3 clients connect to "/ws"
    Then the server tracks 3 connected clients

  Scenario: Disconnected client is removed from tracking
    Given the API server is running
    And a client is connected to "/ws"
    When the client disconnects
    Then the server tracks 0 connected clients

  Scenario: Server ignores messages sent by the client
    Given the API server is running
    And a client is connected to "/ws"
    When the client sends a text message
    Then the connection remains open
    And the server tracks 1 connected client
