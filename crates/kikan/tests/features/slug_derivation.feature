Feature: Slug derivation from free-form display names

  `kikan::slug::derive_slug` is the single entry point for turning a
  free-form display name (legacy `shop_settings.shop_name`, the
  setup-wizard form, the operator-facing profile-create endpoint) into
  a kebab-case `Slug`. The rules are intentionally narrow: no
  transliteration, no auto-correction, no silent re-use of reserved
  names. Anything the rule rejects is the operator's job to rename.

  See `crates/kikan/src/slug.rs` for the algorithm and
  `adr-kikan-upgrade-migration-strategy.md` for why this lives in
  kikan rather than the vertical.

  Scenario Outline: derive_slug accepts and canonicalises valid input
    When I derive a slug from "<input>"
    Then the derived slug is "<expected>"

    Examples:
      | input              | expected           |
      | acme-printing      | acme-printing      |
      | Acme Printing      | acme-printing      |
      | ACME               | acme               |
      | Shop 42 — Main     | shop-42-main       |
      | Acme   &&  Co.     | acme-co            |
      | --Acme--           | acme               |

  Scenario: derive_slug drops non-ASCII bytes without transliteration
    When I derive a slug from "Café"
    Then the derived slug is "caf"

  Scenario: derive_slug rejects an unparseable display name
    When I derive a slug from "!!!"
    Then derive_slug rejects the input as Unparseable

  Scenario: derive_slug rejects an all-whitespace display name
    When I derive a slug from "   "
    Then derive_slug rejects the input as Unparseable

  Scenario: derive_slug rejects an empty display name
    When I derive a slug from ""
    Then derive_slug rejects the input as Unparseable

  Scenario Outline: derive_slug rejects reserved names
    When I derive a slug from "<input>"
    Then derive_slug rejects the input as Reserved "<reserved>"

    Examples:
      | input    | reserved |
      | demo     | demo     |
      | Demo     | demo     |
      | META     | meta     |
      | sessions | sessions |

  Scenario: derive_slug rejects a display name that exceeds the max length
    When I derive a slug from a 61-character ASCII display name
    Then derive_slug rejects the input as TooLong
