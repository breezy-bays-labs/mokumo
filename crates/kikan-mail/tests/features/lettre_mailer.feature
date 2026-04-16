@integration
Feature: LettreMailer delivers to an SMTP server
  As a vertical running in production
  I want LettreMailer to actually transmit mail via SMTP
  So that password recovery and operator notifications land in real inboxes

  Background:
    Given a LettreMailer configured for the local SMTP sink at 127.0.0.1:1025

  Scenario: LettreMailer successfully delivers a text-only message
    When LettreMailer sends an OutgoingMail with text_body "Hello"
    Then the send call returns Ok

  Scenario: LettreMailer surfaces MailError::ConnectFailed when the SMTP server is unreachable
    Given a LettreMailer configured for unreachable host 127.0.0.1:19999
    When LettreMailer sends any OutgoingMail
    Then a ConnectFailed or Transport error is returned

  Scenario: Multipart messages include both text and html bodies
    When LettreMailer sends an OutgoingMail with text_body "plain" and html_body "<p>rich</p>"
    Then the send call returns Ok
