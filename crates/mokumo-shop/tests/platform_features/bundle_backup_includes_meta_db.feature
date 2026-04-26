Feature: Bundle backup primitive captures meta.db alongside profile DBs

  The bundle backup primitive captures every database the caller hands
  it — meta.db, sessions.db, and one snapshot per profile vertical DB.
  Each snapshot is produced via SQLite `VACUUM INTO` so WAL contents
  are checkpointed into a self-contained file. The manifest names every
  snapshot by its caller-supplied logical name so restore can reverse
  the mapping deterministically.

  See `crates/kikan/src/meta/backup.rs` and Q6 in
  `ops/workspace/mokumo/20260425-kikan-meta-db-introduction/validated-requirements.md`.

  Scenario: bundle captures meta, sessions, and a profile DB
    Given a data directory containing meta sessions and one profile database
    When a bundle is created for the data directory
    Then the bundle manifest lists logical names "meta" "sessions" and "vertical-acme"
    And each logical name has a snapshot file on disk
    And every snapshot passes a SQLite integrity check
