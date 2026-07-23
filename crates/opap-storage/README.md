# OPAP Storage

`opap-storage` owns OPAP's local SQLite schema and persistence repositories. It
uses domain-neutral data-transfer objects so importers and user interfaces can
depend on storage without coupling storage to a particular device parser.

The database rejects symlinked database files, enables foreign-key enforcement,
applies numbered migrations on open, and verifies both migration metadata and
referential integrity before use. It supports atomic importer transactions
through `Database::transaction`. `Database::replace_session` is the
authoritative re-import path: it writes session metadata and derived data
together, validates complete waveform coverage, and atomically prunes events,
waveforms, and chunks that disappeared from a newly parsed session.

Schema v8 adds the durable session-snapshot foundation used by the Rust port:

- `session_provenance` preserves the manufacturer therapy day, exact local wall
  boundaries, endpoint UTC offsets and clock corrections, data completeness,
  versioned importer/schema/session-ID algorithms, and opaque source/content
  SHA-256 digests.
- `session_slices` stores ordered, non-overlapping mask-on, mask-off, and
  equipment-off intervals.
- `session_summary` stores exactly one usage value per snapshot, with keyed
  finite numeric values in `summary_metrics`.
- `session_settings` stores exactly one integer, real, text, or boolean value
  per key together with its unit and origin.

`Database::replace_session_snapshot` is the v8 authoritative write API. Before
opening its immediate transaction it rejects non-finite numbers, duplicate
keys or sequences, non-contiguous waveform chunks, invalid session/slice
bounds, overlapping slices, excessive usage, malformed local-time provenance,
and settings with zero or multiple typed values. It then upserts the legacy
`sessions` row and replaces events, waveforms/chunks, and every v8 child as one
atomic unit. A database error leaves the prior session and snapshot unchanged.
`Database::replace_session` remains available for callers that have not yet
adopted snapshot storage. Read complete v8 children through
`Database::session_snapshots`.

This migration intentionally does not mark import jobs complete and does not
add waveform provenance or waveform-segment tables. Those require importer and
service integration in later porting stages.

Import jobs persist explicit `blocked`, `running`, `completed`, `failed`, and
`cancelled` states. Interrupted running jobs recover to blocked, while retries
create time-ordered linked attempts so terminal history is never rewritten.
Persisted import sources and request keys are opaque identifiers rather than
filesystem paths, serials, or caller-provided labels. New jobs require a
service-generated `opap-request:` ID with 32 lowercase hexadecimal characters.
The `opap-request:legacy-<row-id>` form is reserved for privacy-migrated history
and retries that inherit that history; callers cannot create legacy keys. The
v7 rebuild runs with SQLite secure deletion and truncates the WAL after open so
the records it replaces do not remain in those SQLite storage areas; this is
not a general-purpose disk sanitization guarantee.

```sh
rustup run stable cargo test --manifest-path crates/opap-storage/Cargo.toml
```

## Licensing and attribution

OPAP is licensed under GPLv3. It is a derivative of OSCAR and SleepyHead.
Portions are based on SleepyHead, developed and copyrighted by Mark Watkins
(C) 2011-2018, and on work copyrighted by the OSCAR Team. See the repository's
top-level `COPYING` file for the license text.
