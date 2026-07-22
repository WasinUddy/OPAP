# OPAP

OPAP is a modern, local-first CPAP data viewer. Its core is being rewritten in
Rust with behavioral compatibility checked against OSCAR.

The initial implementation covers ResMed card detection and machine
identification. Session, EDF, event, waveform, summary, and database support
will be ported in independently testable slices.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Inspect a ResMed card directory:

```sh
cargo run -p opap-core -- machine-info /path/to/card
```

Private or anonymized conformance cards use the layout documented in
`compat/README.md` and must never be committed accidentally.

## Licensing and attribution

OPAP is a GPLv3 derivative of OSCAR and SleepyHead. Portions are based on the
free and open-source software SleepyHead, developed and copyrighted by Mark
Watkins (C) 2011-2018. Portions of OSCAR are copyright the OSCAR Team.

All redistributed builds must retain the GPLv3 license, source availability,
copyright notices, and this attribution in documentation, installer metadata,
advertising materials where applicable, and the application's About screen.

