Feature: Customer activity log

  The Activity tab on a customer's detail page shows a chronological
  record of every mutation — creation, updates, and archival. This is
  a competitive differentiator: shop owners can audit who changed what
  and when for any customer record.

  Scenario: Activity tab shows a history of customer actions
    Given customer "Acme Printing" has been created and then updated
    When I navigate to the Activity tab for "Acme Printing"
    Then I see activity entries for both actions

  Scenario: Each entry shows the action type and a timestamp
    Given customer "Acme Printing" was recently created
    When I view the Activity tab
    Then the most recent entry shows a "created" action
    And the entry shows a recent timestamp

  Scenario: Update entries describe what changed
    Given customer "Acme Printing" had their email updated
    When I view the Activity tab
    Then I see an update entry that describes the email change

  Scenario: Loading indicator appears while fetching activity
    When I navigate to the Activity tab for a customer
    Then I see a loading skeleton before the entries appear

  Scenario: New customer shows only a creation entry
    Given customer "Fresh Start" was just created
    When I view the Activity tab for "Fresh Start"
    Then I see exactly one activity entry
    And the entry shows a "created" action

  Scenario: Activity list paginates for customers with long histories
    Given customer "Acme Printing" has more activity entries than fit on one page
    When I view the Activity tab
    Then I see pagination controls
    And I can navigate to the next page of activity entries
