# opap-edf

`opap-edf` is OPAP's safe, dependency-free parser for EDF and EDF+ files. It is
designed around files produced by PAP devices and compiles for native Rust and
WebAssembly. Filesystem access and gzip decompression deliberately live outside
this crate.

This is a selective Rust reimplementation, not a claim of complete OSCAR parser
parity. The implementation is informed by two files in the user-selected OSCAR
snapshot, pinned to commit
[`64c5e90a26f91fb15868bcfcccde0c1e1522ac86`](https://gitlab.com/CrimsonNape/OSCAR-code/-/commit/64c5e90a26f91fb15868bcfcccde0c1e1522ac86):

- [`oscar/SleepLib/loader_plugins/edfparser.cpp`](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp),
  SHA-256 `e86ae3953dbda904d12c602a3652bf6445e9eb4cea0ea3b77af810ccaae84086`
- [`oscar/SleepLib/loader_plugins/edfparser.h`](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.h),
  SHA-256 `1f8d55dc5ab4918c5d259e0d3b2cf50d7a2f6432046823dca893ea9883071344`

Those files are copyright (c) 2019-2025 The OSCAR Team and contain work
copyright (c) 2011-2018 Mark Watkins. The pinned OSCAR
[`README`](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/README)
states GPL version 3, while its Debian
[`copyright`](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/Building/Linux/copyright)
metadata labels the upstream files GPL-3+. OSCAR ships its
[`COPYING`](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/COPYING)
terms. This crate declares GPL-3.0-only; see this repository's `COPYING` file.

The pinned OSCAR snapshot has no direct `EDFInfo`/`edfparser` unit tests under
[`oscar/tests`](https://gitlab.com/CrimsonNape/OSCAR-code/-/tree/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/tests).
OPAP's tests are synthetic Rust regressions derived from inspected source
behavior, not a port of an upstream C++ test suite. Private, anonymized device
fixtures and differential tests are still required before claiming broad import
compatibility.

## Verified shared behavior

The synthetic regression suite locks only these source-inspected behaviors:

| Behavior | Pinned OSCAR evidence | OPAP behavior |
| --- | --- | --- |
| Signal count | [`edfparser.cpp` lines 201-209](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L201-209) reject counts outside `1..=256` | Same boundary, plus a configurable smaller upper limit |
| Signal layout | [`edfparser.cpp` lines 233-278](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L233-278) read descriptors in EDF's column-major field order | Same field order |
| Samples | [`edfparser.cpp` lines 319-359](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L319-359) and [489-505](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L489-505) read signed 16-bit little-endian values | Same wire decoding on every target |
| Annotation identification | [`edfparser.cpp` lines 319-324](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L319-324) use the case-sensitive label substring `Annotations` | Same compatibility rule; exact `EDF Annotations` is separately exposed |
| Duplicate labels | [`edfparser.cpp` lines 237-243](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L237-243) retain each signal and append it to the label lookup | Retains them and provides an iterator over exact matches |
| Valid TAL data | [`edfparser.cpp` lines 393-486](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L393-486) accept signed onsets, optional nonnegative durations, multiple texts, and UTF-8 text | Same on well-formed TALs |
| Invalid annotation UTF-8 | [`edfparser.cpp` lines 465-470](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L465-470) pass text through `QString::fromUtf8` | Invalid input is decoded lossily with U+FFFD; exact grouping for every malformed sequence is not claimed |
| Extra payload with a declared record count | [`edfparser.cpp` lines 292-307](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L292-307) require only enough payload, then [decode the declared records](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L319-361) | Decodes the declared records and reports the number of trailing bytes |

These rows do not imply that malformed inputs, dates, calibration, unknown
record counts, or EDF+ timelines produce the same result.

## Intentional differences

OPAP deliberately differs from the pinned OSCAR parser in safety and conformance
behavior:

- Header fields are validated as fixed-width ASCII, numeric conversions are
  checked, allocation/work limits are enforced, and all arithmetic and record
  boundaries are checked. The crate contains no `unsafe` code.
- The declared header byte count must equal `256 + 256 * signal_count`, and
  `parse_header` requires all signal descriptors. OSCAR parses the numeric field
  but does not enforce that equality; its fixed-header helper does not read
  descriptors.
- EDF's `-1` unknown-record sentinel is inferred from a whole-number payload.
  The pinned OSCAR parser treats non-positive record counts as zero decoded
  records.
- OPAP applies the EDF two-digit-year pivot (`85` is 1985) and preserves the
  wire year. The pinned OSCAR correction maps that case to 2085.
- Physical conversion uses `f64` and the complete EDF affine mapping, including
  its additive offset. The pinned parser stores calibration as `float` and sets
  the offset to zero.
- OPAP rejects malformed numeric fields and malformed or unterminated TALs. The
  pinned parser is more permissive and does not expose equivalent structured
  errors.
- EDF+C and EDF+D require an exact primary `EDF Annotations` signal and a
  timekeeping TAL beginning at byte zero with `+Onset 0x14 0x14` (no duration)
  in every record, as required by the
  [EDF+ specification](https://www.edfplus.info/specs/edfplus.html). EDF+C
  record clocks are checked pairwise for contiguity with floating-point
  roundoff tolerance capped at one microsecond. OPAP retains record onsets; the
  pinned parser extracts event texts but does not retain or validate that record
  clock.

For plain EDF and compatibility-mode inputs, labels containing `Annotations`
remain accepted for the verified OSCAR behavior.
`SignalHeader::is_standard_annotation_signal()` lets callers distinguish the
exact EDF+ label.
