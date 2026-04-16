Feature: Mailer trait captures outgoing messages for test assertions
  As a vertical sending password recovery and admin invite emails
  I want a CapturingMailer that records every sent message
  So that BDD scenarios can assert on subject, body, recipients, and headers

  Background:
    Given a CapturingMailer instance

  Scenario: A captured message preserves the OutgoingMail fields
    When send is called with from "no-reply@shop.example" to "owner@shop.example" subject "Reset your password" text_body "Click the link below" html_body "<p>Click the link</p>"
    Then the CapturingMailer reports 1 captured message
    And message 0 has from "no-reply@shop.example"
    And message 0 has to "owner@shop.example"
    And message 0 has subject "Reset your password"

  Scenario: Custom headers are preserved
    When send is called with header "X-Mokumo-Kind" equal to "password-reset"
    Then message 0 has header "X-Mokumo-Kind" equal to "password-reset"

  Scenario: Repeated sends accumulate messages
    When send is called 3 times with the same OutgoingMail
    Then the CapturingMailer reports 3 captured messages

  Scenario: Malformed addresses produce MailError::InvalidAddress
    When an EmailAddress is parsed from "not-an-email"
    Then an InvalidAddress error is returned
