# OSCAR to OPAP port map

OPAP is an intentional GPLv3 Rust port of selected OSCAR behavior. This
document records provenance, compatibility decisions, and implementation
status so translated behavior remains reviewable.

## Pinned upstream

- Repository: <https://gitlab.com/CrimsonNape/OSCAR-code>
- Revision: `64c5e90a26f91fb15868bcfcccde0c1e1522ac86`
- Revision record: [`compat/oscar-code-revision.txt`](compat/oscar-code-revision.txt)
- Inspected: 2026-07-23
- License: GNU GPL version 3; see `COPYING`

At the pinned revision, `resmed_loader.cpp`, `edfparser.cpp`, `edfparser.h`,
`resmed_EDFinfo.cpp`, and `resmed_EDFinfo.h` are byte-for-byte identical to
the files at OSCAR release 1.7.2, commit
`c5c7890785b196993c7c67966f024c32929ec5ab`. The longer master commit above
remains OPAP's canonical oracle identifier.

## Source map

| OPAP module | OSCAR reference | Status |
| --- | --- | --- |
| `opap_core::resmed` identity | `resmed_loader.cpp`: `Detect`, `PeekInfo`, `parseIdentFile`, `parseIdentLine`, `scanProductObject` | Bounded presence detection and identity parsing implemented; the JSON-derived family correction is documented below |
| `opap-edf` | `edfparser.h`, `edfparser.cpp`, `resmed_EDFinfo.*` | Generic EDF/EDF+ parsing implemented with deliberate safety and EDF-spec differences; used for bounded candidate-header inspection and validated uncompressed BRP decoding |
| ResMed candidate index | `resmed_loader.cpp`: `ScanFiles`, `lookupEDFType`, `getEDFDuration`, `ResDayTask::run` | Bounded pre-import heuristic implemented; it is not seeded from `STR.edf` and is not session parity |
| `opap-channels` | `schema.cpp`, `common.cpp`, and ResMed loader aliases | Selected OSCAR-code metadata represented behind typed lookups; `RMVENT_*` entries found only in OSCAR-SQL are excluded from this baseline |
| `opap-analytics` | `session.*`, `day.*`, `common.*`, `machine.*` | Guarded pure helpers implemented but not wired to partial importer output; important differences are documented below |
| ResMed session importer | `resmed_loader.cpp`: `Open`, `ScanFiles`, STR mask records, `LoadBRP`, `LoadPLD`, `LoadSAD`, `LoadEVE`, `LoadCSL` | First bounded library slice implemented for validated uncompressed BRP waveforms only; it emits partial sessions rather than OSCAR-equivalent sessions |
| Session compatibility manifest | `tests/resmedtests.cpp`, `tests/sessiontests.cpp` | Planned; current tests are synthetic or source-derived, and no full-session OSCAR goldens or full-parity suite exists |

The parser, importer, storage, service, native host, and UI do not yet form a
clinical-session import pipeline. The CLI can inspect identity. A direct core
library caller can now use `ResmedImporter::import` for the narrow BRP slice,
but the result is not durably executed by the service, persisted into a user
profile, or queried by the native/UI layers. The service's advertised
`session_import` capability therefore remains `false`. See
[`docs/architecture.md`](docs/architecture.md) for the wiring status.

## Current bounded BRP slice

The core importer discovers and indexes the source before decoding supported
uncompressed `_BRP.edf` files. It validates each complete EDF header against
the bounded index, applies full affine digital-to-physical calibration, and
normalizes supported flow signals to L/min. Device-local timestamps require a
caller-supplied fixed-offset clock context, including explicit clock-correction
provenance; the host timezone is never an implicit input. Emitted session,
source, and waveform keys are deterministic and opaque.

The result is deliberately marked partial. Unsupported or malformed details
produce scoped, privacy-safe warnings or skip an untrustworthy candidate.
Per-file and aggregate bytes, file counts, parser structures, and materialized
samples are all bounded. This is a Rust library capability, not evidence of a
complete OSCAR session or an enabled application import path.

STR mask intervals and settings, PLD detail, EVE events, CSL annotations,
SAD/SA2 oximetry payloads, compressed BRP, STR summary metrics, durable service
execution, native import jobs, and real UI therapy queries remain unavailable.

## Porting rules

1. Preserve upstream notices in translated modules and tests. Mark the Rust
   implementation as modified, with the upstream revision and modification
   date.
2. Treat OSCAR as a pinned behavioral oracle, not as a library linked into the
   production application.
3. Port observable behavior behind typed Rust interfaces. Do not copy Qt UI,
   global profile state, or filesystem/database coupling into the core.
4. Add synthetic unit tests for every translated branch. For whole-card
   behavior, compare OSCAR and Rust output through a deterministic, versioned
   compatibility manifest.
5. Compare identifiers, timestamps, settings, raw values, event ordering, and
   sample counts exactly. Use a documented tolerance only where floating-point
   calculations make exact equality inappropriate.
6. Store full-waveform count and digest in compatibility manifests. OSCAR's
   existing YAML helper truncates long arrays and is not sufficient by itself.
7. Never commit real patient card contents. Public fixtures must be synthetic
   or explicitly approved, demonstrably anonymized, and accompanied by a
   provenance manifest.

## Known upstream compatibility traps

- `Identification.json` takes precedence over `Identification.tgt`; malformed
  JSON does not fall back to TGT.
- OSCAR derives the JSON product family with a truncating string expression.
  OPAP deliberately recognizes known AirSense/AirCurve families robustly while
  preserving serial, product code, and product name verbatim. This correction
  is not byte-for-byte parity.
- OPAP's `DATALOG/` plus `STR.edf` presence check does not mean a source is
  OSCAR-import-ready. OPAP does not parse STR mask records or derive their
  intervals, machine type, or settings. The core BRP slice is narrower than
  OSCAR's import workflow and is not exposed as a native import job.
- Candidate grouping currently models only a bounded EDF-duration overlap
  fallback. It does not use OSCAR's primary STR mask-on/mask-off session seeds;
  compressed EDF (`.edf.gz`), AEV, and unknown DATALOG suffixes may participate
  in grouping, but only validated uncompressed BRP payloads are decoded. CSL
  represents Cheyne-Stokes respiration (CSR) annotations, not central-apnea
  events.
- OSCAR-SQL is a separate fork and is not the pinned oracle. In particular,
  SQL-only `RMVENT_*` channel definitions are outside OPAP's OSCAR-code
  compatibility set.
- OPAP's EDF parser validates and bounds input, uses checked arithmetic, and
  follows standard physical scaling where pinned OSCAR behavior differs. These
  safety/spec corrections require explicit differential expectations rather
  than an assumption of identical output.
- Analytics helpers reject malformed or missing inputs, use checked arithmetic
  and `f64`, and apply a bounded form of OSCAR's day-style duration-weighted
  percentile walk. They do not apply OSCAR's CPAP-machine-type filter or all
  loader/profile session policies. Formula-level source-derived tests are not
  evidence of full analytical parity.
- OSCAR's ResMed test generates YAML but contains no golden assertions and can
  pass with an empty fixture directory.
- Its YAML timestamps depend on the host timezone and event deltas lose
  sub-second precision.
- OSCAR applies ResMed-specific timestamp repair and noon-to-noon day grouping.
- Existing public source does not include the patient-derived ResMed card
  corpus used by OSCAR developers.

There are currently no public or private full-session golden results checked
into OPAP. No module, test, or UI should be described as having full OSCAR
parity until an approved differential corpus exercises the end-to-end importer.

## Attribution

OPAP is based on OSCAR and on the free and open-source software SleepyHead,
developed and copyrighted by Mark Watkins (C) 2011-2018. Portions of OSCAR are
copyright (C) 2019-2025 The OSCAR Team. See `README.md` and `COPYING`.
