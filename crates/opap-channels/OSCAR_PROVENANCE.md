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
| Generic pressure/ramp setting IDs, lookup codes, labels, and units | [`schema.cpp` lines 151–161](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L151-161) |
| Event IDs, lookup codes, labels, schema kinds, and units | [`schema.cpp` lines 162–180](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L162-180) |
| Generic PAP mode ID, labels, and unit | [`schema.cpp` lines 311–324](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/schema.cpp#L311-324) |
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
| ResMed-specific RMS9/RMAS1x setting IDs, labels, and units | [`resmed_loader.cpp` lines 123–288](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L123-288) |
| STR mode, pressure, ramp, EPR, climate, and bilevel setting labels | [`resmed_loader.cpp` lines 1549–2086](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L1549-2086) |
| STR values persisted as session settings | [`resmed_loader.cpp` lines 2576–2695](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/SleepLib/loader_plugins/resmed_loader.cpp#L2576-2695) |
| Legacy `RMS9_SetPressure` ID and labels | [`channels.xml` lines 14–16](https://gitlab.com/CrimsonNape/OSCAR-code/-/blob/64c5e90a26f91fb15868bcfcccde0c1e1522ac86/oscar/docs/channels.xml#L14-16) |

Every one of the 58 registry entries was rechecked against those declarations,
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
- A completed CSL CSR span is stored at its `CSR End` annotation timestamp with
  elapsed seconds since the matching `CSR Start` as the numeric payload. The
  channel's `%` unit remains OSCAR's summary/display unit.
- The generic IPAP and EPAP entries remain sampled-series channels because the
  same OSCAR IDs are persisted from detailed data as well as used in STR
  settings. OSCAR's global alias table makes the `S.BL.*` and `S.S.*` aliases
  available to both PLD and STR dispatch, so OPAP preserves both scopes.
  STR-only extrema, pressure support, ramp, and mode records use the setting
  storage kind.
- `S.PtAccess` maps to different stored setting IDs for pre-AS11 and AS11
  devices, while `Mode` produces both a generic and a raw ResMed mode. Those
  duplicate, model-dependent raw mappings are deliberately alias-free so the
  exact resolver remains unambiguous.
- `RMAS1x_EasyBreathe` is declared and used by the loader but is never assigned
  a `ChannelID` at this revision. OPAP does not infer that the unused `0xe211`
  gap belongs to it.

SAD/SA2 oximetry, STR summary statistics, machine identity, summary-only
channels, and other device families remain outside this crate's scope. No
source EDF fixture or loader result is bundled here, so this crate claims
metadata-level source fidelity only. End-to-end parity requires anonymized or
synthetic EDF fixtures exercised through the importer.
