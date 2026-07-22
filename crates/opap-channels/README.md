# opap-channels

`opap-channels` is OPAP's small, explicit PAP channel registry. It is pure
Rust, `no_std` + `alloc`, serde-ready, and portable to `wasm32-unknown-unknown`.
Its legacy metadata and ResMed alias evidence are pinned to OSCAR-code commit
`64c5e90a26f91fb15868bcfcccde0c1e1522ac86`.

The durable identity of a channel is a stable string such as
`pap.event.obstructive_apnea` or `pap.series.leak_rate`. OSCAR numeric
`ChannelID` values are exposed only through the visibly named
`LegacyOscarChannelId` compatibility type. New OPAP code must not persist an
OSCAR number as its only channel identity.

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

The registry includes only:

- the five PAP event annotations persisted by the pinned ResMed EVE loader;
- the three BRP signals accepted by that loader;
- PLD signals that the pinned loader actually persists; and
- OSCAR's device-reported apnea compatibility channel plus pressure/leak roles
  that match OPAP's current analytics inputs.

The stored ResMed alias list transcribes the relevant rows from the pinned
translation table. `resmed_signal` is intentionally stricter than OSCAR's
loader matcher. OSCAR uses case-insensitive prefix matching; OPAP's canonical
metadata lookup requires an exact, case-sensitive alias and fails closed if a
mapping is ambiguous.
File-family scope is part of an alias identity: for example, `Mask Pres` means
the high-rate mask-pressure channel in BRP and the regular mask-pressure channel
in PLD. Importers that need legacy permissiveness must implement and test that
policy at their boundary rather than silently broadening this registry API.

`Unit::EventsPerHour` is the aggregate/display unit for event channels. An
individual EVE event carries the EDF annotation duration in seconds when one is
present; the pinned parser uses `-1.0` when it is absent, and the loader forwards
that value. Each accepted record is counted once. Keeping these facts separate
prevents a payload from being mislabeled as an events-per-hour measurement.

`ChannelDto` is an owned transport snapshot and can deserialize untrusted data.
Use `ChannelDto::registered_definition` to resolve its stable key and obtain
canonical metadata. `ChannelDto::is_canonical_snapshot` checks every field when
an exact registry snapshot is required.

## Deliberate omissions

- CSL/CSR spans, SAD/SA2 oximetry, STR settings, machine type/settings,
  summary-only channels, and other device families are outside this crate's
  initial scope. Clear-airway events come from EVE annotations; CSL carries
  CSR span annotations and is not a source for clear-airway events.
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
