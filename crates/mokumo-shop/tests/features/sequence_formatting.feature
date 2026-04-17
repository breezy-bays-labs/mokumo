Feature: Sequence Number Formatting

  Display numbers follow a consistent format across all business
  entities: a prefix, a hyphen separator, and a zero-padded value.
  This format is recognizable to shop owners and suitable for
  printed documents, invoices, and verbal communication.

  Scenario Outline: Format display number
    Given a prefix "<prefix>" and padding <padding>
    When value <value> is formatted
    Then the display number is "<expected>"

    Examples:
      | prefix | padding | value | expected   |
      | C      | 4       | 1     | C-0001     |
      | INV    | 6       | 42    | INV-000042 |
      | C      | 4       | 10000 | C-10000    |
      | Q      | 2       | 999   | Q-999      |
      | C      | 4       | 0     | C-0000     |

  Scenario: Empty prefix produces hyphen-prefixed number
    Given a prefix "" and padding 4
    When value 1 is formatted
    Then the display number is "-0001"
