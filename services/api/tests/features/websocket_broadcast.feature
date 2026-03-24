Feature: WebSocket broadcast

  Scenario: Connected client receives a broadcast event
    Given the API server is running
    And a client is connected to "/ws"
    When a "customer.created" event is broadcast
    Then the client receives a message with type "customer.created"
    And the message has version 1
    And the message has topic "customer"

  Scenario: All connected clients receive the same broadcast
    Given the API server is running
    And 3 clients are connected to "/ws"
    When a "customer.created" event is broadcast
    Then all 3 clients receive the message

  Scenario: Broadcast event contains the full payload
    Given the API server is running
    And a client is connected to "/ws"
    When a "customer.created" event is broadcast with payload '{"id": 1, "name": "Test Customer"}'
    Then the client receives a message with type "customer.created"
    And the message payload contains "id" with value 1
    And the message payload contains "name" with value "Test Customer"

  Scenario: Broadcast with no connected clients does not block
    Given the API server is running
    And no clients are connected
    When a "customer.created" event is broadcast
    Then the broadcast completes without error
