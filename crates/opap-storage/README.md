# OPAP Storage

`opap-storage` owns OPAP's local SQLite schema and persistence repositories. It
uses domain-neutral data-transfer objects so importers and user interfaces can
depend on storage without coupling storage to a particular device parser.

The database enables foreign-key enforcement, applies numbered migrations on
open, and supports atomic importer transactions through `Database::transaction`.
`Database::replace_session_data` is the authoritative re-import path: it
validates complete waveform coverage and atomically prunes events, waveforms,
and chunks that disappeared from a newly parsed session.

```sh
rustup run stable cargo test --manifest-path crates/opap-storage/Cargo.toml
```

## Licensing and attribution

OPAP is licensed under GPLv3. It is a derivative of OSCAR and SleepyHead.
Portions are based on SleepyHead, developed and copyrighted by Mark Watkins
(C) 2011-2018, and on work copyrighted by the OSCAR Team. See the repository's
top-level `COPYING` file for the license text.
