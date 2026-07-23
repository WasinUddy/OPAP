# Canonical compatibility manifest v1

`schema/opap-compat-manifest.schema.json` is the interchange contract between
an OSCAR oracle export and the corresponding OPAP export. The Rust loader also
uses `serde(deny_unknown_fields)` and performs checks that JSON Schema cannot
express: exact counts, canonical order, source-record sequences, registered
channel metadata, endpoint time consistency, session bounds, slice and segment
coverage, preview overlap, aggregate digests, and finite floats.

## Oracle and producer provenance

Every document pins this oracle identity exactly:

```text
name: OSCAR
repository: https://gitlab.com/CrimsonNape/OSCAR-code
revision: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
export_schema_version: opap-oscar-oracle/v1
```

The left/expected manifest must have producer role `oracle`, name `OSCAR`, and
that exact OSCAR-code source revision. The right/actual manifest must have role
`subject`, name `OPAP`, and the full 40-character OPAP commit tested. Reversed
or same-role comparisons are errors.

Each producer also records an HTTPS adapter repository, full adapter revision,
`adapter_tree_sha256`, `adapter_conformance_sha256`, and `adapter_clean: true`.
`synthetic_fixture_only` is suitable when `fixture.synthetic` is `true`. Every
real-card manifest (`fixture.synthetic: false`) must instead use
`verified_clean_tree`. The verifier hashes `git archive --format=tar HEAD` for
each clean adapter tree and prints the digest of the checked-in conformance
vector set.

These fields are attestations made by the exporter; the manifest cannot prove
its own build provenance. Independently reproduce the output of
`scripts/verify-compat-trees.sh`, review the adapter revision, and confirm that
both adapters actually passed the public vectors. A manifest that merely
copies those strings is not trusted evidence.

## Required top-level identity

`schema_version` is exactly `opap-oscar-compat/v1`; unknown versions and
unknown fields fail closed. `fixture` always contains a canonical `case_id`, a
lowercase source SHA-256, and the exact `synthetic` boolean used by the adapter
attestation rule.

`machine` always contains `manufacturer`, `model`, `model_number`,
`serial_number`, `firmware`, and `sha256`. Manufacturer, model, and serial
number must be nonempty; model number and firmware remain exact strings even
when the source leaves them empty. All machine fields and the source-tree
digest compare exactly.

## Canonical order and identity

Arrays have the following order:

- sessions by `source_id_sha256`;
- summary metrics and settings by `key`;
- event and waveform channels by `channel_id`;
- slices, events, and waveform segments in exact zero-based contiguous
  `sequence` order.

Canonical identifiers contain lowercase ASCII letters, digits, underscore,
hyphen, and dot, and start with a letter or digit. This makes bytewise ordering
portable across Rust, C++, and JavaScript.

`session_id` is an independent stable key. It is not a timestamp and must not
be synthesized by formatting `start_utc`. `session.source_id_sha256` separately
commits to the source-side session identity and is the canonical array order
and comparison key. Once paired by source identity, `session_id` compares
exactly as an independent field.

Event order is the stable source-record order. Each event carries an exact
`sequence` and `source_id_sha256`; adapters must not sort events by value or
time after decoding. `offset_milliseconds` is an exact nonnegative integer.
`duration_milliseconds` may be a nonnegative integer or `null`, and `value` may
be a finite number or `null`. Null presence compares exactly. Only present
event values use `event_value_abs`; offset and duration never use a tolerance.

As an adapter-side requirement, when OSCAR exposes an event offset or present
duration in seconds, the adapter normalizes it once as `milliseconds =
round(seconds * 1000)`, with exact half-way ties rounded away from zero. It
rejects a non-finite or negative input and any multiplication/conversion
overflow. A missing source duration stays `null`; it must not be rewritten as
zero. The loader sees only the normalized integer, so each adapter must cover
this conversion with its own tests/vectors. The normalized integer is then the
exact comparison value.

Event and waveform channel identifiers, canonical units, storage kind, and
ResMed source-file kind (`eve`, `brp`, or `pld`) must match the pinned OPAP
channel registry. An event channel's `source_file_kind` is exactly `eve`.
Waveform channels use the channel's registered `brp` or `pld` mapping. A
declared waveform channel has at least one sample and one segment. Segment
sample ranges exactly cover the emitted stream without sample gaps; exact start
offsets preserve source timing gaps while temporal overlap is rejected.

Settings use one of four tagged value objects, with no implicit conversion:

```json
{"type":"number","value":6.0}
{"type":"integer","value":2}
{"type":"boolean","value":true}
{"type":"text","value":"autoset"}
```

Number values must be finite and use `setting_number_abs`; integer values are
JSON-safe, and boolean/text values compare exactly. The setting key and unit
always compare exactly.

## Required versus empty collections

The manifest always contains the session summary, settings, events, and
waveforms objects with their declared count and digest. Summary metrics,
settings, event channels, events within an explicitly declared channel, and
waveform channels may be empty. Sessions and slices may not be empty, and an
emitted waveform channel may not have an empty sample stream.

An empty collection is not a wildcard. Keyed comparison reports a missing
member when the oracle contains a session, metric, setting, or channel that the
subject omits. Ordered arrays likewise produce an incompatible length or field
mismatch when slices, events, or segments are omitted. A missing digest or
collection field is an invalid manifest before comparison begins.

## Time and session slices

UTC timestamps use exactly `YYYY-MM-DDTHH:MM:SS.mmmZ`; local wall timestamps
use exactly `YYYY-MM-DDTHH:MM:SS.mmm` with no suffix. The three millisecond
digits are mandatory. Start and end have independent nullable
`*_utc_offset_seconds` fields and independent exact signed
`*_clock_correction_milliseconds` fields. For each endpoint whose offset is not
`null`, validation requires:

```text
utc_epoch_milliseconds
  = local_wall_epoch_milliseconds
  - (utc_offset_seconds * 1000)
  + clock_correction_milliseconds
```

When an endpoint offset is `null`, the exporter is explicitly saying that the
source supplied no offset, so the cross-interpretation equation cannot be
checked for that endpoint; the UTC and local spellings still compare exactly.
`timezone_basis` is exactly `source_endpoint_metadata`. V1 does not claim an
IANA time-zone identity or a pinned time-zone database. Endpoint offsets may
differ; each present offset is inclusively bounded to `-64800..64800` seconds.
Local wall time need not be monotonic across a daylight-saving fallback, but
`end_utc` must be later than `start_utc`.

Every session has one or more non-overlapping source-ordered slices. A slice
records an exact source identity digest, one of `mask_on`, `mask_off`, or
`equipment_off`, and exact start/end millisecond offsets within the session.
`summary.usage_milliseconds` must equal the sum of all `mask_on` slice
durations and must not exceed the UTC session duration. Summary metric keys and
units compare exactly; a metric value may be `null` or a finite number.

All count, sequence, sample, placement, duration, and usage integers are in the
cross-language JSON safe-integer range. The loader and Draft 2020-12 schema use
mathematical integer semantics, so integral JSON spellings such as `3`, `3.0`,
and `3e0` represent the same integer; a fractional value does not.

EDF decimal header values are at most eight bytes and use canonical plain
decimal spelling: no exponent, leading plus, redundant leading zero, negative
zero, empty fraction, or trailing fractional zero. For example, `-3276.8`,
`0.5`, and `1` are canonical; `-03276.8`, `.5`, `-0`, and `1.0` are not.

A waveform sample rate must be finite and positive and must agree with
`samples_per_record / record_duration_decimal` within
`waveform_sample_rate_hz_abs`. Encoding digital minimum and maximum both fit
signed 16-bit values and the minimum is strictly smaller. Physical minimum and
maximum use the canonical EDF decimal grammar and are strictly ordered.
`samples_per_record` is in `1..=u32::MAX`, and
`record_duration_decimal` is a strictly positive canonical EDF decimal.

## Digest contract

Every digest is required lowercase SHA-256 and compares exactly. The
comparator recomputes the slice, event-collection, waveform-collection, and
session aggregate commitments below. It cannot recompute a raw-source or full
waveform digest from manifest previews, so those child commitments require
independent adapter tests.

Source digests use these adapter-side rules:

- `fixture.source_sha256` is SHA-256 of a deterministic card-tree stream. For
  each regular file in strict bytewise UTF-8 relative-path order, use the
  `source_tree_sha256` preimage defined below. Reject symlinks and path escapes.
- `machine.sha256` uses the same construction over the identification files
  consumed for machine identification. `session.source_sha256` uses it over
  the selected source-file set for the session.
- `settings.sha256`, `summary.source_sha256`, and each event-channel `sha256`
  commit to their ordered raw source records with `record_stream_sha256` below.
- Each waveform `source_sha256` hashes the exact selected raw EDF signal bytes.
  Each segment also records the exact source digest for its selected segment.
- `source_id_sha256` fields are deterministic commitments to the adapter's
  source identity bytes. They are exact opaque identities, not hashes of
  normalized timestamps or display values, and need adapter-specific vectors.

The harness validates the shape and exact equality of these source commitments
but does not have the source bytes needed to reproduce them. A checked-in,
reviewed adapter test must prove every applicable rule above before its
attestation is credible.

`record_stream_sha256` hashes this exact byte preimage, preserving the
producer-selected record order:

```text
ASCII("opap-source-record-stream-v1") || 0x00
u64be(record_count)
for each record:
  u64be(record_byte_length)
  record_bytes
```

The empty record stream therefore includes the domain and `u64be(0)`; it is
not SHA-256 of zero bytes. The public vector contains the two exact UTF-8
records `first-record` and `second-record\n` (lengths 12 and 14) and produces:

```text
ff0ca4fdd81b491a60b760d07a70d47efa4d2f393d696f4e6ba45bb5b6e93c94
```

`source_tree_sha256` hashes this exact byte preimage:

```text
ASCII("opap-source-tree-v1") || 0x00
u64be(entry_count)
for each entry in strict canonical path order:
  u32be(path_utf8_byte_length)
  path_utf8_bytes
  u64be(content_byte_length)
  content_bytes
```

A canonical source path is nonempty and relative, uses `/` separators, has no
leading `/`, backslash, NUL, empty component, `.` component, or `..` component,
and is strictly sorted without duplicates. The helper enforces path spelling
and order; the adapter must additionally reject symlinks and filesystem path
escapes before providing bytes. The public vector contains
`Identification.json` followed by `STR.edf` and produces:

```text
7d71e686b6b8490f367c9739fd069257ff26228a2ed647fa471eb8cf6a53cfdf
```

Both cases are specified in `tests/fixtures/source-digest-vector.json` and are
executed by the Rust unit tests.

### Structured aggregate preimages

Structured commitments are SHA-256 of the UTF-8 bytes of the shown compact
JSON arrays. Their values are ASCII strings and JSON-safe integers, so this is
also their RFC 8785 JCS representation. No spaces, line endings, or trailing
newline are included. Inputs must already be in the canonical order above.

The exact public slice preimage and result are:

```text
["opap-slice-collection-v1",[[0,"23bb05ff493a095a575870b61c01b691c52f7f32d89b9637c104aaf64d4849a1","mask_on",0,27000000]]]
sha256 = aa37f32998289adf0be813fa6f1aa1c260d98e3e2f8490c390d27c7261556c8d
```

In general, each inner slice entry is `[sequence, source_id_sha256, status,
start_offset_milliseconds, end_offset_milliseconds]`.

The exact public event-collection preimage and result are:

```text
["opap-event-collection-v1",[["pap.event.obstructive_apnea","3e4339e7845d7c5881b9b9a92001f84a26c1c1267bafda151be2fedf3d982f06"]]]
sha256 = d1d6af6136d33c50809398957b5007f09ea70c97c3608a5a5803673117901100
```

In general, each inner event entry is `[channel_id,
event_channel_sha256]`. The empty collection preimage is exactly
`["opap-event-collection-v1",[]]`.

The exact public waveform-collection preimage and result are:

```text
["opap-waveform-collection-v1",[["pap.series.flow_rate","0442fdcd640f5125d8bf60dcaf2b3249912440148ebde22910a2135600e15a8e","fd36c987511bffb9f93237bc87f56e8a5ca22d50117dab98ceaa9ac9e95813a4"]]]
sha256 = adf0cce471b2352873ae8510dcbc728a7c3cf0a51ca7abe1f77b5d8cc62c6e9e
```

Each inner waveform entry is `[channel_id, source_sha256,
semantic_sha256]`. The empty collection preimage is exactly
`["opap-waveform-collection-v1",[]]`.

The exact public session aggregate preimage and result are:

```text
["opap-session-aggregate-v1","3dc99fe74efd88c53459e93de1e65a2912047740db3edabb79e17f2efc195145","191f86388cf82898383ae7449a767deb620445e897a9ba8dfccf7c15eb2b0f9a","aa37f32998289adf0be813fa6f1aa1c260d98e3e2f8490c390d27c7261556c8d","42e0a0142ef0d61dc16963eff0e1da37e622e3c72563b23c1f163cb4b534f457","e9d5800da38b0a6e90a7c25441551e745a68e87a68be50f7ff715e294a6007a4","d1d6af6136d33c50809398957b5007f09ea70c97c3608a5a5803673117901100","adf0cce471b2352873ae8510dcbc728a7c3cf0a51ca7abe1f77b5d8cc62c6e9e"]
sha256 = d7352cde2fd063465b3188a1ff3509e38b16eb5c26d70353f6868ddb87de0ea0
```

Its fields after the tag are, in order, `session.source_id_sha256`,
`session.source_sha256`, `slices.sha256`, `summary.source_sha256`,
`settings.sha256`, `events.sha256`, and `waveforms.sha256`.

### Waveform semantic preimage v2

The manifest encoding kind is exactly `edf-i16le-f32be-segments-v1`. Its
semantic SHA-256 preimage uses the domain tag
`opap-waveform-semantic-edf-i16-f32-segments-v2` followed by a zero byte. It
binds source provenance, exact temporal placement, every decoded digital
sample, and every final importer-emitted physical `f32` sample. `str(s)` below
means `u64be(UTF-8 byte length) || UTF-8 bytes`; a SHA value means the decoded
32 digest bytes, not 64 hexadecimal characters.

```text
ASCII("opap-waveform-semantic-edf-i16-f32-segments-v2") || 0x00
str(channel_id)
str(source_file_kind)                     # "eve", "brp", or "pld"
str(unit)
bytes32(source_sha256)
i64be(start_offset_milliseconds)
u64be(declared_sample_count)
str(encoding.kind)
i32be(encoding.digital_min)
i32be(encoding.digital_max)
str(encoding.physical_min_decimal)
str(encoding.physical_max_decimal)
u32be(encoding.samples_per_record)
str(encoding.record_duration_decimal)
u64be(segment_count)
for each segment:
  u64be(sequence)
  u64be(start_sample)
  u64be(sample_count)
  i64be(start_offset_milliseconds)
  bytes32(segment.source_sha256)
u64be(decoded_sample_count)
for each decoded sample: i16le(sample)
u64be(emitted_physical_sample_count)
for each emitted physical sample: u32be(f32::to_bits(sample))
```

Digital and physical stream lengths must match the declared sample count.
Physical samples must be finite. The exact `f32` bit commitment means `-0.0`
and `0.0` are distinct, and adapters must hash the final emitted stream rather
than recomputing values from the raw EDF during export.

`tests/fixtures/waveform-digest-vector.json` deliberately uses the non-zero
affine scale `physical = digital * 0.5 + 1.0`, represented by digital bounds
`-32768..32767` and physical bounds `-16383..16384.5`. It includes two
temporally separated segments, interior samples, and both digital extrema. Its
exact semantic digest is:

```text
fd36c987511bffb9f93237bc87f56e8a5ca22d50117dab98ceaa9ac9e95813a4
```

Only the first and last `min(sample_count, 16)` physical values appear as
manifest previews. Overlapping head/tail entries must be exactly consistent
within one manifest, but cross-producer preview comparison uses its named
tolerance. The semantic digest still covers the complete exact digital and
physical streams, so a tolerant preview cannot hide an interior mismatch.

## Comparison policy

Schema/oracle identity, fixture and machine metadata, session/source identity,
timestamps and endpoint metadata, slices, counts, array membership, keys,
units, setting types, integers, booleans, text, event millisecond placement and
duration, nullable presence, waveform encoding and placement, and all digests
compare exactly. Only these five named absolute tolerances exist:

| Name | Absolute tolerance | Field |
|---|---:|---|
| `summary_metric_abs` | `1e-6` | present session-summary metric values |
| `setting_number_abs` | `1e-6` | numeric setting values |
| `event_value_abs` | `1e-6` | present event values |
| `waveform_sample_rate_hz_abs` | `1e-9` | waveform sample rate |
| `waveform_preview_sample_abs` | `1e-4` | head/tail preview samples |

An absolute difference equal to the tolerance passes; a larger or non-finite
difference fails. Run the CLI's `tolerances` subcommand to print the compiled
profile. A digest mismatch fails even when a visible value is within tolerance.
Diagnostics identify paths and tolerance names while redacting values, private
paths, and keyed identifiers.

## Scope boundary

V1 compares each imported session as an independent record. A manifest can
contain multiple sessions, but it defines no aggregation across them. It does
not define OSCAR noon-boundary/day assignment, daily grouping or rollups,
cross-session slice union, or cross-session analytics such as daily AHI/RDI and
percentiles. Those behaviors are deferred until separate, pinned oracle vectors
exist. Nothing in this format supports a claim of full OSCAR parity or medical
correctness.
