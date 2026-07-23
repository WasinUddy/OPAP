# opap-channels

`opap-channels` is OPAP's small, explicit PAP channel registry. It is pure
Rust, `no_std` + `alloc`, serde-ready, and portable to `wasm32-unknown-unknown`.
Its legacy metadata and ResMed alias evidence are pinned to OSCAR-code commit
`64c5e90a26f91fb15868bcfcccde0c1e1522ac86`.

The durable identity of a channel is a stable string such as
`pap.event.obstructive_apnea`, `pap.series.leak_rate`, or
`oximetry.series.oxygen_saturation`. OSCAR numeric `ChannelID` values are
exposed only through the visibly named `LegacyOscarChannelId` compatibility
type. New OPAP code must not persist an OSCAR number as its only channel
identity. Oximetry uses its own durable namespace because pulse and oxygen
saturation are not PAP measurements, even when they arrive beside ResMed PAP
data.

```rust
use opap_channels::{
    LegacyOscarChannelId, ResmedFileKind, by_legacy_id, by_stable_key,
    resmed_signal,
};

let leak = by_stable_key("pap.series.leak_rate").expect("registered");
assert_eq!(leak.unit.symbol(), "L/min");
assert_eq!(
    by_legacy_id(LegacyOscarChannelId(0x1108)),
    Some(leak),
);
assert_eq!(
    resmed_signal(ResmedFileKind::Pld, "Leak.2s"),
    Some(leak),
);
```

## Scope

The registry includes:

- the five PAP event annotations persisted by the pinned ResMed EVE loader;
- the three BRP signals accepted by that loader;
- PLD signals that the pinned loader actually persists;
- the CSL `CSR Start`/`CSR End` pair used to build Cheyne Stokes respiration
  spans;
- pulse-rate and oxygen-saturation signals from the equivalent ResMed SAD and
  SA2 oximetry paths;
- foundational STR pressure, ramp, mode, EPR, climate, comfort, and bilevel
  settings persisted by `StoreSettings`; and
- OSCAR's device-reported apnea compatibility channel plus pressure/leak roles
  that match OPAP's current analytics inputs.

The stored ResMed alias list transcribes the relevant rows from the pinned
translation table. `resmed_signal` is intentionally strict: the canonical
metadata lookup requires an exact, case-sensitive alias and fails closed if a
mapping is ambiguous. Importer boundaries can use `resmed_signal_prefix` for
OSCAR's label-starts-with-alias direction and case-insensitive comparison. That
resolver also fails closed when aliases from multiple channels match.
File-family scope is part of an alias identity: for example, `Mask Pres` means
the high-rate mask-pressure channel in BRP and the regular mask-pressure channel
in PLD. STR-only setting labels do not resolve as PLD, EVE, or CSL data. SAD
and SA2 remain distinct provenance kinds even though the pinned loader sends
both through the same oximetry decoder and translation rows. OSCAR's shared
translation table is an explicit exception: the IPAP/EPAP `S.BL.*` and `S.S.*`
aliases are accepted by both PLD and STR dispatch.

Some pinned setting metadata is deliberately alias-free. `S.PtAccess` becomes
either the S9/10 patient-access setting or the AirSense/AirCurve 11 patient-view
setting depending on machine generation, and the same `Mode` source is stored
as both OSCAR's generic PAP mode and ResMed's raw mode. Registering those labels
twice would make exact resolution ambiguous, so only the unambiguous canonical
mapping is exposed.

The permissive resolver is locale-independent. It compares the Unicode
lowercase forms of characters without normalization. This matches the pinned
OSCAR behavior for the ASCII BRP/EVE labels while remaining deterministic for
non-ASCII aliases; exact equivalence with every Qt Unicode case-folding edge
case is not claimed.

`Unit::EventsPerHour` is the aggregate/display unit for event channels. An
individual EVE event carries the EDF annotation duration in seconds when one is
present; the pinned parser uses `-1.0` when it is absent, and the loader forwards
that value. Each accepted record is counted once. Keeping these facts separate
prevents a payload from being mislabeled as an events-per-hour measurement.

`Unit::Percent` is likewise the summary/display unit for the CSL CSR channel.
The loader pairs `CSR Start` with `CSR End`, stores the completed span at the end
timestamp, and attaches elapsed seconds as its payload. `SpanSemantics` and
`resmed_span_endpoint_role` preserve those endpoint and payload facts.

`ChannelDto` is an owned transport snapshot and can deserialize untrusted data.
Use `ChannelDto::registered_definition` to resolve its stable key and obtain
canonical metadata. `ChannelDto::is_canonical_snapshot` checks every field when
an exact registry snapshot is required.

## Deliberate omissions

- STR summary statistics, machine identity, summary-only channels, oximetry
  signals beyond SAD/SA2 pulse and SpO2, and other device families remain
  outside this scope. Clear-airway events come from EVE annotations; CSL
  carries CSR spans and is not a source for clear-airway events.
- The pinned loader declares `RMAS1x_EasyBreathe` and can store it, but never
  assigns it a `ChannelID`. This registry does not manufacture the apparent
  missing `0xe211` value. `RMS9_TubeType` is also unregistered and not persisted
  by `StoreSettings`.
- The pinned PLD loader recognizes I:E labels but its persistence call is
  commented out; this registry therefore does not claim I:E support.
- The loader explicitly skips `AlvMinVent.2s`, `CLRatio.2s`, and
  `TRRatio.2s`; they are not aliases or channels in this registry. Their
  `RMVENT` IDs from another repository are intentionally not imported here.
- Unknown/diagnostic fields (`Va`, `RMS9_E01`, `RMS9_E02`, and CRC fields) are
  not channels here.
- Derived AHI/RDI series are not source channels. The registry marks count
  inputs only; analytics owns the formulas.
- No leak redline, diagnostic cutoff, treatment target, or recommendation is
  encoded. The registry describes device records, not medical meaning.

See [OSCAR_PROVENANCE.md](OSCAR_PROVENANCE.md) for pinned source evidence and
[NOTICE.md](NOTICE.md) for copyright attribution.
