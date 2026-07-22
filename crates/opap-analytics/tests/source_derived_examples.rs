#![allow(clippy::float_cmp)]

//! Hand-worked examples derived from the pinned OSCAR source.
//!
//! Source revision: `CrimsonNape/OSCAR-code`
//! `64c5e90a26f91fb15868bcfcccde0c1e1522ac86`.
//!
//! These tests do not build or execute OSCAR C++, consume no oracle output, and
//! are not differential compatibility tests. See `OSCAR_PROVENANCE.md`.

use opap_analytics::{
    AhiEventCounts, CivilDate, CorrectedLocalTimestampMs, SessionUsageInput, TherapyDay,
    WeightedSample, calculate_event_indices, summarize_therapy_usage,
    therapy_day_from_local_epoch_ms, weighted_percentile,
};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

#[test]
fn source_derived_cpap_day_overlap_example_uses_nonzero_timestamps() {
    // Pinned source: SleepLib/day.cpp:757-830 unions already-selected enabled
    // ranges. Nonzero timestamps avoid copying OSCAR's zero-sentinel edge case.
    let base = 10 * DAY_MS;
    let sessions = [
        SessionUsageInput::new(base, base + 2 * HOUR_MS, vec![]),
        SessionUsageInput::new(base + HOUR_MS, base + 3 * HOUR_MS, vec![]),
    ];
    let usage = summarize_therapy_usage(&sessions).expect("valid source-derived input");
    assert_eq!(usage.therapy_ms, 10_800_000);
}

#[test]
fn source_derived_ahi_and_rdi_positive_time_example() {
    // Pinned source: SleepLib/schema.cpp:413-424 selects five AHI channels;
    // SleepLib/day.h:249-263 divides their count by positive CPAP hours and
    // adds RERA only for RDI.
    let counts = AhiEventCounts {
        clear_airway: 2,
        obstructive_apnea: 3,
        hypopnea: 4,
        unclassified_apnea: 1,
        device_reported_apnea: 0,
        rera: 2,
    };
    let result = calculate_event_indices(counts, 18_000_000).expect("positive therapy time");
    assert_eq!(result.ahi, 2.0);
    assert_eq!(result.rdi, 2.4);
}

#[test]
fn source_derived_day_weighted_percentile_interior_example() {
    // Pinned source: SleepLib/day.cpp:345-474 interpolates weighted value-bin
    // midpoints. With four equal bins, its ordinary interior p50 is 2.5.
    // SleepLib/common.h:111-134 independently gives 2.5 for its plain median.
    // This is not OSCAR Session::percentile parity: for one event list, that
    // separate unweighted nth_element path selects 3 for [1, 2, 3, 4] at p50;
    // its multi-list copy behavior differs again.
    let samples = [
        WeightedSample::new(1.0, 40),
        WeightedSample::new(2.0, 40),
        WeightedSample::new(3.0, 40),
        WeightedSample::new(4.0, 40),
    ];
    assert_eq!(
        weighted_percentile(&samples, 0.5).expect("valid interior percentile"),
        2.5
    );
}

#[test]
fn source_derived_default_noon_local_time_example() {
    // Pinned source: after Qt has produced local date/time,
    // SleepLib/machine.cpp:257-285 assigns a start before the configured split
    // to the previous date; SleepLib/profiles.h:705-723 defaults that split to
    // noon. The Rust input is explicitly corrected naive-local time.
    let jan_2 = TherapyDay::from_civil_date(CivilDate::new(1970, 1, 2).expect("valid date"))
        .expect("valid key");
    let before_noon = DAY_MS + 11 * HOUR_MS + 59 * 60_000;
    let at_noon = DAY_MS + 12 * HOUR_MS;
    assert_eq!(
        therapy_day_from_local_epoch_ms(CorrectedLocalTimestampMs::from_naive_local_epoch_ms(
            before_noon,
        ))
        .days_since_unix_epoch(),
        jan_2.days_since_unix_epoch() - 1
    );
    assert_eq!(
        therapy_day_from_local_epoch_ms(CorrectedLocalTimestampMs::from_naive_local_epoch_ms(
            at_noon,
        )),
        jan_2
    );
}
