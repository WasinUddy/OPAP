# ADR 0001: local-first health-data architecture

- Status: accepted
- Date: 2026-07-22

## Context

OPAP imports identifiable CPAP device records, including therapy sessions,
respiratory events, timestamps, device identifiers, and possibly user-entered
profile data. Uploading these records creates privacy, consent, retention, and
breach risks that are unnecessary for the core viewer.

## Decision

OPAP is local-first:

- Import, parsing, calculation, storage, search, visualization, backup, and
  export run on the user's device by default.
- The application MUST work without an account or Internet connection.
- OPAP MUST NOT upload health data, filenames, device identifiers, database
  contents, reports, or crash dumps without a separate, explicit user action.
- User data is stored under the operating system's per-user application-data
  directory with least-privilege permissions. OPAP relies on operating-system
  account and full-disk encryption for at-rest protection until application
  encryption is separately designed and shipped.
- The UI and Rust core exchange typed, versioned data. Raw card access and
  database writes remain in the trusted native boundary; browser/WASM modules
  receive only the bytes required for a requested pure parse or calculation.
- Network features, if added, MUST be isolated behind an interface that can be
  disabled and tested. Sync is out of scope until it has a separate threat
  model, encryption design, consent flow, and deletion policy.

## Consequences

Users control their own backups and exports. OPAP cannot recover lost local
data. Device compromise, shared OS accounts, unsafe exports, and unencrypted
backups remain user-visible risks. Local-first behavior and default-off network
access are release invariants, not merely UI preferences.
