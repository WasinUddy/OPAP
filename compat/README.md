# OSCAR compatibility harness

This directory contains a strict, test-only differential contract for the
Rust rewrite. The behavioral oracle is
[`CrimsonNape/OSCAR-code`](https://gitlab.com/CrimsonNape/OSCAR-code) at commit
`64c5e90a26f91fb15868bcfcccde0c1e1522ac86`; the resolution record is
`oscar-code-revision.txt`. `CrimsonNape/OSCAR-SQL` is a different Git history
and must not be substituted for this pin.

The checked-in manifests are wholly synthetic. They test the schema,
validation, deterministic comparator, source/aggregate/waveform digest vectors,
CLI exit behavior, and redacted diagnostics. They are not output from a
completed OPAP card importer and are not evidence of full OSCAR parity.

Run the standalone harness independently of the production Cargo workspace:

```sh
cargo test --manifest-path compat/Cargo.toml
cargo run --manifest-path compat/Cargo.toml -- \
  compare \
  compat/tests/fixtures/synthetic-oscar.json \
  compat/tests/fixtures/synthetic-opap.json
cargo run --manifest-path compat/Cargo.toml -- tolerances
```

The machine-readable contract is
`schema/opap-compat-manifest.schema.json`. `FORMAT.md` specifies the semantic
checks, ordering, exact digest preimages, and five named float tolerances.
`PRIVATE_ORACLE.md` describes how to compare a private card without committing
OSCAR source, patient data, or generated manifests to this repository.

## What v1 compares

The expected document must be an OSCAR `oracle` export and the actual document
an OPAP `subject` export. Both identify the same pinned oracle and fixture.
Within each session, v1 covers independent stable session identity, UTC and
local endpoint metadata, session slices, source-provided session summary
metrics, settings, events, waveform placement and segments, channel registry
metadata, counts, and provenance/semantic digests.

Sessions and session slices must be non-empty. Other required collections may
be explicitly empty when the source genuinely has no members: summary metrics,
settings, event channels, the events within a declared channel, and waveform
channels. The collection object, count, and digest must still be present. A
valid empty subject collection is nevertheless incompatible when the oracle
contains an expected member. Missing sessions, expected channels, expected
metrics/settings, array entries, or any required digest never produce a pass.

All timestamps, millisecond event placement and duration, identities, counts,
units, segment placement, and digests compare exactly. Only the five fields
listed in `FORMAT.md` use a named absolute tolerance. In particular, event
offsets and optional durations are exact integers; they are not float-tolerant.

## Differential test strategy

1. Pin and build the OSCAR-code revision above in an external checkout. Run its
   relevant C++ tests and keep `oscar/tests/resmedtests.cpp`,
   `oscar/tests/sessiontests.cpp`, and the ResMed loader behavior as reference
   material; do not copy OSCAR source into OPAP.
2. Commit the non-PHI oracle and subject export adapters in their own
   repositories. For real-card work, verify clean OSCAR, OPAP, and adapter
   trees with `compat/scripts/verify-compat-trees.sh`.
3. Give both programs the same read-only synthetic or private card. Export every
   required non-waveform record to `opap-oscar-compat/v1`, and feed each complete
   digital and emitted physical waveform stream into its semantic digest. The
   manifest intentionally carries only the prescribed waveform head/tail
   previews plus that full-stream digest. Do not derive the oracle manifest
   from OPAP or truncate any settings, summaries, events, slices, or segments.
4. Validate both documents, then compare OSCAR as the left/expected manifest
   and OPAP as the right/actual manifest. Preserve a reviewed oracle result as
   immutable test evidence. Never change it merely to make a failure pass.
5. Add public regression fixtures only when they are generated and provably
   non-PHI. Keep all real-card inputs and outputs outside Git.

An adapter attestation is still a claim inside the manifest. A green comparison
is credible only when the adapter revision, clean-tree archive digest, and
conformance-vector digest have been independently reproduced. Passing proves
only that the two pinned exporters agree for the compared fixtures and v1
fields. It does not prove medical correctness, support for every device or
firmware, or complete OSCAR feature parity.

## Deliberate v1 boundary

V1 is a session-level import-compatibility harness. A manifest may contain
multiple sessions, but v1 treats each as an independent record. It deliberately
defers OSCAR's noon-boundary/day assignment, daily rollups, cross-session slice
unions, and cross-session analytics such as daily AHI/RDI and percentiles. The
`session.summary` object compares source-emitted per-session values; the harness
does not treat them as a substitute for a separately tested analytics engine.
Those behaviors need their own pinned oracle vectors before any parity claim.

## Existing private machine check

The separate ignored `opap-core` conformance test still supports a private
machine-identification corpus:

```sh
OPAP_RESMED_FIXTURES="$PWD/testdata/private/resmed" \
  cargo test -p opap-core --test resmed_manual_expectations -- --ignored
```

That narrow machine check is not the canonical session differential harness.
Raw real-world CPAP cards must never be committed.
