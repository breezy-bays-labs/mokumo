Feature: Customer discovery tabs

  The Contacts, Artwork, Pricing, and Communication tabs display mock
  data during M0 to establish the tab navigation pattern. Real data will
  replace these mocks as each domain ships in later milestones. All tabs
  must render content, not empty states — this validates the vertical
  pattern is complete end-to-end.

  Scenario: Contacts tab shows mock contact cards
    When I navigate to the Contacts tab for a customer
    Then I see contact cards with names and role badges

  Scenario: A primary contact is indicated
    When I view the Contacts tab for a customer
    Then one contact card is flagged as the primary contact

  Scenario: Contact cards show contact details
    When I view the Contacts tab for a customer
    Then each contact card shows an email address and phone number

  Scenario: Artwork tab shows a placeholder gallery
    When I navigate to the Artwork tab for a customer
    Then I see a gallery grid with placeholder artwork items

  Scenario: Pricing tab shows placeholder pricing templates
    When I navigate to the Pricing tab for a customer
    Then I see pricing template cards

  Scenario: Communication tab shows a placeholder timeline
    When I navigate to the Communication tab for a customer
    Then I see a timeline of placeholder messages
