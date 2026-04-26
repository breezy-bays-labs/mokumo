Feature: Bundle restore refuses on any partial corruption (strict atomic)

  When a bundle's manifest is missing or any snapshot file fails
  integrity, the restore primitive refuses to proceed and leaves every
  destination file on disk unchanged. There is no "best-effort partial
  restore" — operators get a clear refusal naming the failed file
  rather than a partially-restored state.

  See `crates/kikan/src/meta/backup.rs`, R6 in
  `ops/workspace/mokumo/20260425-kikan-meta-db-introduction/shaping.md`,
  and `adr-kikan-upgrade-migration-strategy.md` §"Multi-database
  operation-level atomicity via snapshot-and-restore".

  Scenario: one corrupt snapshot in a bundle aborts restore with disk unchanged
    Given a snapshots directory with a bundle group containing 3 healthy database snapshots
    And the snapshot for "vertical-acme" is corrupted on disk
    And destination database files exist with known content
    When the bundle group is restored to the data directory
    Then restore fails with BundleRestoreError PartialCorruption naming "vertical-acme"
    And every destination database file is byte-identical to its pre-restore content

  Scenario: missing manifest aborts restore with disk unchanged
    Given a snapshots directory with a bundle group containing 3 healthy database snapshots
    And the bundle manifest is removed
    And destination database files exist with known content
    When the bundle group is restored to the data directory
    Then restore fails with BundleRestoreError ManifestVerificationFailed
    And every destination database file is byte-identical to its pre-restore content
