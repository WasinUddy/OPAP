# OSCAR channel provenance

Reference repository: [`CrimsonNape/OSCAR-code`](https://gitlab.com/CrimsonNape/OSCAR-code)

Pinned commit: [`64c5e90a26f91fb15868bcfcccde0c1e1522ac86`](https://gitlab.com/CrimsonNape/OSCAR-code/-/commit/64c5e90a26f91fb15868bcfcccde0c1e1522ac86)

License: GNU General Public License v3

The legacy metadata and ResMed mappings in this crate are a Rust transcription
of evidence in that exact source revision. The implementation does not compile
or call OSCAR's C++ code. The test snapshot is hand-maintained and checks only
the source-derived metadata plus OPAP's own stable keys and roles; it is not an
OSCAR executable fixture or an end-to-end compatibility claim.

## Evidence map

| Registry fact | Pinned OSCAR source |
|---|---|
| Copyright notices and GPL terms on the source files | [`schema.cpp` lines 1–8](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L1-8), [`resmed_loader.cpp` lines 1–8](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L1-8) |
| Pressure-series IDs, lookup codes, labels, schema kinds, and units | [`schema.cpp` lines 134–150](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L134-150) |
| Event IDs, lookup codes, labels, schema kinds, and units | [`schema.cpp` lines 162–180](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L162-180) |
| Common waveform IDs, labels, schema kinds, and units | [`schema.cpp` lines 230–278](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L230-278) |
| Exact lookup-code constants used by the waveform declarations | [`common_gui.h` lines 16–32](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/common_gui.h#L16-32) |
| Exact default English unit strings | [`common.cpp` lines 744–770](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/common.cpp#L744-770) |
| Five AHI-contributing legacy channels | [`schema.cpp` lines 413–424](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L413-424) |
| RERA is added to the AHI channel count for RDI | [`day.h` lines 251–262](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/day.h#L251-262) |
| EDF annotation duration is optional and represented as `-1.0` when absent | [`edfparser.cpp` lines 420–453](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/edfparser.cpp#L420-453) |
| ResMed EVE annotation timestamp, accepted event channels, and duration handling | [`resmed_loader.cpp` lines 3260–3314](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3260-3314) |
| BRP channel selection and flow conversion to litres per minute | [`resmed_loader.cpp` lines 3360–3417](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3360-3417) |
| PLD persisted channels, leak conversion to litres per minute, and tidal-volume conversion to millilitres | [`resmed_loader.cpp` lines 3546–3673](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3546-3673) |
| `AlvMinVent.2s`, `CLRatio.2s`, and `TRRatio.2s` are explicitly skipped | [`resmed_loader.cpp` lines 3674–3681](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3674-3681) |
| OSCAR's permissive case-insensitive prefix matcher | [`resmed_loader.cpp` lines 3940–3955](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3940-3955) |
| Exact EVE/BRP/PLD signal-alias table | [`resmed_loader.cpp` lines 3957–4015](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3957-4015) |
| CSL carries CSR start/end spans rather than clear-airway events | [`resmed_loader.cpp` lines 3144–3211](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L3144-3211) |

Every one of the 22 registry entries was rechecked against those declarations,
unit constants, loader branches, and alias rows at the pinned revision.

## Compatibility boundaries

- OPAP stable keys are new names and are **not** OSCAR lookup codes. The exact
  OSCAR lookup code remains separately available in `legacy_oscar.lookup_code`.
- `resmed_signal` is a canonical metadata resolver, not a byte-for-byte port of
  `matchSignal`: it deliberately requires an exact case-sensitive alias and
  returns no result on ambiguity. The alias rows are source-derived; the lookup
  policy is intentionally narrower and fail-closed.
- `LegacyOscarChannelId` values reproduce the pinned source; they are not an
  allocation range for new OPAP channels.
- OPAP canonical unit symbols normalize capitalization (`L/min`, `mL`, `s`,
  `events/h`). Exact OSCAR English unit strings are retained separately in
  `legacy_oscar.unit_label` for metadata checks. The OSCAR pressure unit is
  exactly `cmH2O`.
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
- `AlvMinVent.2s`, `CLRatio.2s`, and `TRRatio.2s` are explicitly ignored by
  this OSCAR-code revision. The three `RMVENT` IDs formerly copied from a
  different repository are excluded from the registry.
- Clear-airway events are sourced from EVE's `Central apnea` annotation. CSL
  parses `CSR Start`/`CSR End` spans and must not be treated as a source of
  clear-airway events.

CSL/CSR spans, SAD/SA2 oximetry, machine type/settings, STR settings,
summary-only channels, and other device families remain outside this crate's
scope. No source EDF fixture or loader result is bundled here, so this crate
claims metadata-level source fidelity only. End-to-end parity requires
anonymized or synthetic EDF fixtures exercised through the importer.
