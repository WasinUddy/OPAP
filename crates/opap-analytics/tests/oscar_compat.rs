#![allow(clippy::float_cmp)]

use opap_analytics::{
    AhiEventCounts, CivilDate, CorrectedLocalTimestampMs, SessionUsageInput, TherapyDay,
    WeightedSample, calculate_event_indices, summarize_therapy_usage,
    therapy_day_from_local_epoch_ms, weighted_percentile,
};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

#[test]
fn oscar_day_usage_overlap_example() {
    // Pinned provenance: SleepLib/day.cpp:684-758 unions overlapping session
    // ranges. [0h, 2h) plus [1h, 3h) therefore represents 3h, not 4h.
    let sessions = [
        SessionUsageInput::new(0, 2 * HOUR_MS, vec![]),
        SessionUsageInput::new(HOUR_MS, 3 * HOUR_MS, vec![]),
    ];
    let usage = summarize_therapy_usage(&sessions).expect("valid OSCAR example");
    assert_eq!(usage.therapy_ms, 10_800_000);
}

#[test]
fn oscar_ahi_and_rdi_arithmetic_example() {
    // Pinned provenance: SleepLib/schema.cpp:415-425 selects five AHI channels;
    // SleepLib/day.h:249-263 divides their sum by CPAP hours and adds RERA only
    // for RDI.
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
fn oscar_even_median_reliable_interior_example() {
    // Pinned provenance: SleepLib/common.h:131-153 averages the two central
    // values; the weighted rank path in session.cpp:2357-2472 agrees here.
    let samples = [
        WeightedSample::new(1.0, 40),
        WeightedSample::new(2.0, 40),
        WeightedSample::new(3.0, 40),
        WeightedSample::new(4.0, 40),
    ];
    assert_eq!(
        weighted_percentile(&samples, 0.5).expect("valid median"),
        2.5
    );
}

#[test]
fn oscar_noon_day_assignment_example() {
    // Pinned provenance: SleepLib/machine.cpp:359-375 assigns corrected local
    // starts before the configured split to the previous date; profiles.cpp:698
    // sets the OSCAR default to noon.
    let jan_2 = TherapyDay::from_civil_date(CivilDate::new(1970, 1, 2).expect("valid date"))
        .expect("valid key");
    let before_noon = DAY_MS + 11 * 3_600_000 + 59 * 60_000;
    let at_noon = DAY_MS + 12 * 3_600_000;
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
