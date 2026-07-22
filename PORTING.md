# OSCAR to OPAP port map

OPAP is an intentional GPLv3 Rust port of selected OSCAR-SQL behavior. This
document records provenance, compatibility decisions, and implementation
status so translated behavior remains reviewable.

## Pinned upstream

- Repository: <https://gitlab.com/CrimsonNape/OSCAR-SQL>
- Revision: `3741e5b423e4b5796c51a9d447e83b2525963d50`
- Inspected: 2026-07-22
- License: GNU GPL version 3; see `COPYING`

## Source map

| OPAP module | OSCAR reference | Status |
| --- | --- | --- |
| `opap_core::resmed` | `resmed_loader.cpp`: `Detect`, `PeekInfo`, `parseIdentFile`, `parseIdentLine`, `scanProductObject` | Detection and identification ported |
| `opap-edf` | `edfparser.h`, `edfparser.cpp`, `resmed_EDFinfo.*` | Generic EDF/EDF+ parser implemented and independently tested; ResMed integration not started |
| ResMed session importer | `resmed_loader.cpp`: `Open`, `ScanFiles`, `ResDayTask`, `LoadBRP`, `LoadPLD`, `LoadSAD`, `LoadEVE`, `LoadCSL` | Planned |
| Session compatibility manifest | `tests/resmedtests.cpp`, `tests/sessiontests.cpp` | Schema and oracle harness planned |
| Derived calculations | `calcs.*`, `session.*` | Planned after import parity |

The parser, importer, storage, service, native host, and UI are not an
end-to-end pipeline yet. The current CLI stops after machine identification;
`ResmedImporter::import` deliberately reports an unsupported operation. See
[`docs/architecture.md`](docs/architecture.md) for the wiring status.

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
- OSCAR's ResMed test generates YAML but contains no golden assertions and can
  pass with an empty fixture directory.
- Its YAML timestamps depend on the host timezone and event deltas lose
  sub-second precision.
- OSCAR applies ResMed-specific timestamp repair and noon-to-noon day grouping.
- The C++ EDF parser uses a zero physical offset in a compatibility-sensitive
  path even though standard EDF scaling normally includes an offset.
- Existing public source does not include the patient-derived ResMed card
  corpus used by OSCAR developers.

## Attribution

OPAP is based on OSCAR and on the free and open-source software SleepyHead,
developed and copyrighted by Mark Watkins (C) 2011-2018. Portions of OSCAR are
copyright (C) 2019-2026 The OSCAR Team. See `README.md` and `COPYING`.
