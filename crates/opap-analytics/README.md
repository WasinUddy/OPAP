# opap-analytics

Pure Rust, deterministic summary calculations for OPAP's daily and overview UI.
The crate has no runtime dependencies and performs no I/O. It can run natively
or as `wasm32-unknown-unknown`.

It currently provides:

- unioned active-therapy usage across overlapping sessions and mask-on slices;
- per-hour event components, AHI, and RDI from explicit event counts;
- finite-only min/max, weighted mean, median, p90, p95, and p99.5;
- named leak (L/min) and pressure (cmH2O) summaries without unit conflation;
- noon-to-noon therapy-day assignment with explicit UTC offset and device-time
  correction inputs; and
- deterministic aggregation of enabled sessions into daily summaries.

The calculations describe imported device data. They do not diagnose a sleep
condition, recommend settings, or replace review by a qualified clinician.

## Time and weighting contracts

Intervals are half-open `[start_ms, end_ms)`. When a session has slices, only
`MaskOn` slices count; with no slices, the session window counts. All active
intervals are unioned so overlap is not double-counted.

Each `WeightedSample` pairs a finite physical value with the positive integer
milliseconds it represents. For regular samples, pass the exact integer sample
period to `summarize_regular_signal`. When rates vary, allocate the represented
milliseconds explicitly; never use raw sample count as a proxy for time.

The noon helper consumes a `CorrectedLocalTimestampMs`, a distinct type for a
corrected **naive-local** epoch timestamp. Use
`therapy_day_from_normalized_utc` with core's already-normalized UTC value; the
caller must supply the offset that applied at that instant. Use
`therapy_day_from_raw_device_local_epoch_ms` when device correction has not yet
been applied. The crate has no timezone database and does not guess
daylight-saving transitions.

```rust
use opap_analytics::{
    AhiEventCounts, SessionUsageInput, WeightedSample, calculate_event_indices,
    summarize_session_usage, summarize_signal,
};

let usage = summarize_session_usage(&SessionUsageInput::new(
    0,
    7_200_000,
    vec![],
))?;
let indices = calculate_event_indices(
    AhiEventCounts {
        obstructive_apnea: 2,
        hypopnea: 1,
        ..AhiEventCounts::default()
    },
    usage.therapy_ms,
)?;
assert_eq!(indices.ahi, 1.5);

let pressure = summarize_signal(&[
    WeightedSample::new(8.0, 40),
    WeightedSample::new(9.0, 40),
])?;
assert_eq!(pressure.median, 8.5);
# Ok::<(), opap_analytics::AnalyticsError>(())
```

## Source provenance, not a parity claim

[`OSCAR_PROVENANCE.md`](OSCAR_PROVENANCE.md) pins every source-derived formula
to `CrimsonNape/OSCAR-code` commit
`64c5e90a26f91fb15868bcfcccde0c1e1522ac86` and documents the exact behavior
differences. The examples in `tests/source_derived_examples.rs` are
hand-worked source examples, not differential-oracle or compatibility tests.

In particular, OPAP bounds percentiles instead of copying OSCAR's upper-tail
extrapolation/out-of-range behavior; rejects zero-time, overflow, and malformed
overlapping inputs; combines signal means by represented sample duration rather
than complete session duration; and implements only a fixed noon split with an
explicit time/offset contract. It does not implement OSCAR's unweighted session
percentile or its optional session-combining policies. No end-to-end parity is
claimed.

Before real ResMed sessions use `aggregate_by_noon`, integration must carry the
core candidate's authoritative `resmed_day` through aggregation. Recomputing a
day from a header-selected timestamp can move a drifted near-noon file to the
wrong day; see the integration gate in the provenance document.
