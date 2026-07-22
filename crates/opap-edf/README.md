# opap-edf

`opap-edf` is OPAP's safe, dependency-free parser for EDF and EDF+ files. It is
designed around the files produced by ResMed devices and can be compiled for
native Rust or WebAssembly. Filesystem access and gzip decompression deliberately
live outside this crate.

The implementation is an idiomatic Rust port informed by the EDF behavior in
OSCAR's `SleepLib/loader_plugins/edfparser.*`. OSCAR is copyright the OSCAR Team;
the original SleepyHead work is copyright Mark Watkins. This crate is distributed
under GPL-3.0-only. See the repository's `COPYING` file.

The parser validates all fixed-width fields, checked arithmetic, allocation
limits, record boundaries, little-endian samples, EDF affine calibration, and
EDF+ TAL annotations. Discontinuous EDF+D record onsets are retained from the
leading empty timekeeping TAL. Decode work, annotation objects, annotation text,
and dense record metadata all have configurable limits, and parser-controlled
variable allocations use fallible reservation. It contains no `unsafe` code.

## Conformance policy

The parser is deliberately strict where ambiguity would corrupt the timeline:
EDF+D files must contain a primary signal labeled exactly `EDF Annotations`,
and every discontinuous record must begin with an empty timekeeping TAL. Missing
clocks are returned as structured errors rather than inferred from record index.

For compatibility with OSCAR and existing PAP exports, continuous files also
recognize case-sensitive signal labels containing `Annotations`, accept trailing
bytes after a known declared record count, and decode invalid annotation UTF-8
with replacement characters. Callers can distinguish the strict standard label
using `SignalHeader::is_standard_annotation_signal()`.
