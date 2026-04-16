Feature: Event bus fan-out with broadcast semantics
  As a kikan vertical emitting typed events
  I want multiple subscribers to receive each event
  And I want slow subscribers to not block publishers
  So that lifecycle, health, and migration signals can drive the UI and observability

  Background:
    Given a BroadcastEventBus with default capacity

  Scenario: Multiple subscribers each receive a published LifecycleEvent
    Given subscriber "ui" is subscribed to LifecycleEvent
    And subscriber "observability" is subscribed to LifecycleEvent
    When LifecycleEvent::Serving is published
    Then subscriber "ui" receives LifecycleEvent::Serving
    And subscriber "observability" receives LifecycleEvent::Serving

  Scenario: Events of one type are not delivered to subscribers of another type
    Given subscriber "lifecycle-only" is subscribed to LifecycleEvent
    And subscriber "health-only" is subscribed to HealthEvent
    When LifecycleEvent::ShutdownInitiated is published
    Then subscriber "lifecycle-only" receives LifecycleEvent::ShutdownInitiated
    And subscriber "health-only" receives no HealthEvent within 50ms

  Scenario: Subscribing after a publish does not retroactively deliver historical events
    When LifecycleEvent::BootStarted is published
    And subscriber "late" subscribes to LifecycleEvent
    Then subscriber "late" receives no LifecycleEvent within 50ms

  Scenario: A slow subscriber does not block a publisher
    Given subscriber "fast" is subscribed to HealthEvent
    When 2048 HealthEvent::GreenToYellow events are published
    Then every publish completes without blocking
    And subscriber "fast" receives the latest HealthEvent

  Scenario: Dropping all receivers does not panic the publisher
    Given subscriber "only" is subscribed to MigrationEvent
    When subscriber "only" drops its MigrationEvent receiver
    And MigrationEvent::Completed is published
    Then the publish completes without error

  Scenario: Concurrent publishers on the same event type
    Given subscriber "collector" is subscribed to ProfileEvent
    When 10 tasks each publish a distinct ProfileEvent::Switched concurrently
    Then subscriber "collector" receives exactly 10 ProfileEvent events
