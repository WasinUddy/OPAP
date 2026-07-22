# OSCAR channel provenance

Reference repository: `CrimsonNape/OSCAR-SQL`
Pinned commit: `3741e5b423e4b5796c51a9d447e83b2525963d50`
License: GNU General Public License v3

The compatibility metadata and ResMed mappings in this crate are a Rust port
of evidence in that exact source revision. The implementation does not compile
or call the C++ code.

## Evidence map

| Registry fact | Pinned OSCAR source |
|---|---|
| Numeric IDs, lookup codes, English labels, short labels, schema kinds, and units for common PAP events/series | [`oscar/SleepLib/schema.cpp` lines 141–294](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/schema.cpp#L141-294) |
| Exact default English unit strings | [`oscar/SleepLib/common.cpp` lines 850–883](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/common.cpp#L850-883) |
| Five AHI-contributing legacy channels | [`oscar/SleepLib/schema.cpp` lines 415–425](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/schema.cpp#L415-425) |
| EDF annotation duration is optional and represented as `-1.0` when absent | [`edfparser.cpp` lines 402–451](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/loader_plugins/edfparser.cpp#L402-451) |
| ResMed EVE annotation timestamp and duration handling | [`resmed_loader.cpp` lines 3770–3831](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3770-3831) |
| BRP channel selection and flow conversion to litres/minute | [`resmed_loader.cpp` lines 3852–3941](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3852-3941) |
| PLD persisted channels, leak conversion to litres/minute, and tidal-volume conversion to millilitres | [`resmed_loader.cpp` lines 4034–4227](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L4034-4227) |
| Exact EVE/BRP/PLD signal aliases | [`resmed_loader.cpp` lines 4504–4554](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L4504-4554) |
| OSCAR's permissive case-insensitive prefix matcher | [`resmed_loader.cpp` lines 4473–4488](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L4473-4488) |
| ResMed ventilation IDs, labels, and units | [`resmed_loader.cpp` lines 304–316](https://gitlab.com/CrimsonNape/OSCAR-SQL/-/blob/3741e5b423e4b5796c51a9d447e83b2525963d50/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L304-316) |

## Compatibility boundaries

- OPAP stable keys are new names and are **not** OSCAR lookup codes. The exact
  OSCAR lookup code remains separately available in `legacy_oscar.lookup_code`.
- `resmed_signal` is a canonical metadata resolver, not a byte-for-byte port of
  `matchSignal`: it deliberately requires an exact case-sensitive alias and
  returns no result on ambiguity. The alias table is source-compatible; the
  lookup policy is intentionally narrower and fail-closed.
- `LegacyOscarChannelId` values reproduce the pinned source; they are not an
  allocation range for new OPAP channels.
- OPAP canonical unit symbols normalize capitalization (`L/min`, `mL`, `s`,
  `events/h`). Exact OSCAR English unit strings are retained separately in
  `legacy_oscar.unit_label` for differential checks.
- ResMed's EVE loader stores each annotation's duration as its event value (or
  the parser's `-1.0` sentinel when omitted), but event summaries count
  occurrences per therapy hour. The registry represents both facts rather than
  conflating their units.
- `Press.2s` follows the pinned loader's therapy-pressure mapping. The upstream
  comment notes that the label can also be used in an IPAP context; this crate
  preserves the actual mapping and makes no additional mode inference.
- A ResMed hypopnea annotation may have no duration according to the pinned
  loader comment. The registry exposes the missing-duration case and does not
  manufacture a replacement duration.

No source fixture or loader result is bundled here, so this crate claims
metadata-level compatibility only. End-to-end parity requires anonymized or
synthetic EDF fixtures exercised through the importer.
