use std::collections::BTreeMap;

use crate::{
    AhiEventCounts, AnalyticsError, EventIndices, LeakPressureSummary, SessionUsageInput,
    UsageSummary, WeightedSample, calculate_event_indices, summarize_leak_and_pressure,
    summarize_therapy_usage,
};

const MILLIS_PER_SECOND: i64 = 1_000;
const MILLIS_PER_DAY: i64 = 86_400_000;
const NOON_MILLIS: i64 = 43_200_000;
const UNIX_EPOCH_ADJUSTMENT_DAYS: i64 = 719_468;

/// A validated proleptic-Gregorian calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl CivilDate {
    /// Construct a validated proleptic-Gregorian date.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError::InvalidCivilDate`] for an invalid month or day.
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, AnalyticsError> {
        let date = Self { year, month, day };
        if day == 0 || day > days_in_month(year, month).unwrap_or(0) {
            return Err(AnalyticsError::InvalidCivilDate { year, month, day });
        }
        Ok(date)
    }
}

/// The named therapy day as days since 1970-01-01.
///
/// This fixed-noon contract corresponds to OSCAR's pinned default/forced `ResMed`
/// split. It does not model OSCAR's configurable split or combining policies.
/// The day begins at local noon on the named civil date and ends immediately
/// before local noon the next date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TherapyDay {
    days_since_unix_epoch: i64,
}

impl TherapyDay {
    #[must_use]
    pub const fn from_days_since_unix_epoch(days: i64) -> Self {
        Self {
            days_since_unix_epoch: days,
        }
    }

    #[must_use]
    pub const fn days_since_unix_epoch(self) -> i64 {
        self.days_since_unix_epoch
    }

    /// Convert a validated calendar date to a therapy-day key.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError::InvalidCivilDate`] if public fields were mutated
    /// into an invalid date before conversion.
    pub fn from_civil_date(date: CivilDate) -> Result<Self, AnalyticsError> {
        // Revalidate because CivilDate's fields are public for ergonomic DTO use.
        let date = CivilDate::new(date.year, date.month, date.day)?;
        Ok(Self::from_days_since_unix_epoch(days_from_civil(date)))
    }

    /// Convert this key back to a proleptic-Gregorian date.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError::ArithmeticOverflow`] when an arbitrary key's
    /// year cannot fit in `i32`.
    pub fn to_civil_date(self) -> Result<CivilDate, AnalyticsError> {
        civil_from_days(self.days_since_unix_epoch)
    }
}

/// A corrected naive-local epoch timestamp, distinct from a UTC instant.
///
/// This type prevents daily aggregation from silently interpreting normalized
/// UTC milliseconds as local wall-clock milliseconds near the noon boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CorrectedLocalTimestampMs(i64);

impl CorrectedLocalTimestampMs {
    /// Wrap milliseconds whose calendar/time fields already represent corrected local time.
    #[must_use]
    pub const fn from_naive_local_epoch_ms(value: i64) -> Self {
        Self(value)
    }

    /// Return the naive-local epoch-millisecond representation.
    #[must_use]
    pub const fn as_i64(self) -> i64 {
        self.0
    }

    /// Convert an already-normalized UTC instant using its applicable offset.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError::ArithmeticOverflow`] if applying the UTC
    /// offset exceeds the `i64` timestamp range.
    pub fn from_normalized_utc(
        normalized_utc_timestamp_ms: i64,
        utc_offset_seconds: i32,
    ) -> Result<Self, AnalyticsError> {
        let offset_ms = i64::from(utc_offset_seconds)
            .checked_mul(MILLIS_PER_SECOND)
            .ok_or(AnalyticsError::ArithmeticOverflow(
                "converting UTC offset to milliseconds",
            ))?;
        normalized_utc_timestamp_ms
            .checked_add(offset_ms)
            .map(Self)
            .ok_or(AnalyticsError::ArithmeticOverflow(
                "applying UTC offset to normalized timestamp",
            ))
    }

    /// Apply device-clock correction to an uncorrected naive-local timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError::ArithmeticOverflow`] if applying the device
    /// correction exceeds the `i64` timestamp range.
    pub fn from_raw_device_local_epoch_ms(
        raw_device_local_timestamp_ms: i64,
        device_correction_ms: i64,
    ) -> Result<Self, AnalyticsError> {
        raw_device_local_timestamp_ms
            .checked_add(device_correction_ms)
            .map(Self)
            .ok_or(AnalyticsError::ArithmeticOverflow(
                "correcting raw device-local timestamp",
            ))
    }
}

/// Assign a corrected, naive-local timestamp to the noon-to-noon therapy day.
///
/// `local_timestamp_ms` uses Unix-epoch arithmetic but represents local wall
/// time. Times before 12:00 belong to the preceding named date. Euclidean
/// division keeps pre-1970 timestamps correct.
#[must_use]
pub fn therapy_day_from_local_epoch_ms(
    local_timestamp_ms: CorrectedLocalTimestampMs,
) -> TherapyDay {
    let local_timestamp_ms = local_timestamp_ms.as_i64();
    let calendar_day = local_timestamp_ms.div_euclid(MILLIS_PER_DAY);
    let millis_since_midnight = local_timestamp_ms.rem_euclid(MILLIS_PER_DAY);
    let therapy_day = if millis_since_midnight < NOON_MILLIS {
        // For an i64 millisecond timestamp, calendar_day is many orders of
        // magnitude away from i64::MIN, so this subtraction is representable.
        calendar_day - 1
    } else {
        calendar_day
    };
    TherapyDay::from_days_since_unix_epoch(therapy_day)
}

/// Apply an offset to an already-normalized UTC instant, then assign its day.
///
/// `utc_offset_seconds` must be the offset applicable at this timestamp; this
/// pure crate intentionally does not infer daylight-saving transitions. Device
/// correction must not be applied here because normalized UTC already includes it.
///
/// # Errors
///
/// Returns [`AnalyticsError::ArithmeticOverflow`] if applying the offset
/// exceeds the `i64` range.
pub fn therapy_day_from_normalized_utc(
    normalized_utc_timestamp_ms: i64,
    utc_offset_seconds: i32,
) -> Result<TherapyDay, AnalyticsError> {
    CorrectedLocalTimestampMs::from_normalized_utc(normalized_utc_timestamp_ms, utc_offset_seconds)
        .map(therapy_day_from_local_epoch_ms)
}

/// Correct a raw naive device-local timestamp, then assign its noon therapy day.
///
/// # Errors
///
/// Returns [`AnalyticsError::ArithmeticOverflow`] if applying the device-clock
/// correction exceeds the `i64` range.
pub fn therapy_day_from_raw_device_local_epoch_ms(
    raw_device_local_timestamp_ms: i64,
    device_correction_ms: i64,
) -> Result<TherapyDay, AnalyticsError> {
    CorrectedLocalTimestampMs::from_raw_device_local_epoch_ms(
        raw_device_local_timestamp_ms,
        device_correction_ms,
    )
    .map(therapy_day_from_local_epoch_ms)
}

/// One enabled-or-disabled session contribution to daily analytics.
///
/// `local_start` is used only for day assignment. `usage` timestamps share
/// any consistent absolute timeline and are used only for interval union.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionAnalyticsInput {
    pub enabled: bool,
    pub local_start: CorrectedLocalTimestampMs,
    pub usage: SessionUsageInput,
    pub events: AhiEventCounts,
    pub leak_lpm: Vec<WeightedSample>,
    pub pressure_cmh2o: Vec<WeightedSample>,
}

/// Deterministic daily output sorted by therapy-day key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DailySummary {
    pub day: TherapyDay,
    pub usage: UsageSummary,
    pub event_counts: AhiEventCounts,
    pub event_indices: Option<EventIndices>,
    pub signals: LeakPressureSummary,
}

/// Group enabled sessions by their corrected local start and aggregate each day.
///
/// Therapy intervals are unioned across sessions, event counts are summed with
/// overflow checks, and signal samples retain their explicit time weights.
/// Disabled sessions are ignored. A day with events but no active therapy is
/// rejected instead of producing NaN or infinity.
///
/// # Errors
///
/// Returns an error for invalid intervals/signals, arithmetic overflow, or
/// events attached to a day without positive active-therapy time.
pub fn aggregate_by_noon(
    sessions: &[SessionAnalyticsInput],
) -> Result<Vec<DailySummary>, AnalyticsError> {
    let mut groups: BTreeMap<TherapyDay, DailyAccumulator> = BTreeMap::new();

    for session in sessions.iter().filter(|session| session.enabled) {
        let day = therapy_day_from_local_epoch_ms(session.local_start);
        let group = groups.entry(day).or_default();
        group.usage.push(session.usage.clone());
        group.events = group.events.checked_add(session.events)?;
        group.leak_lpm.extend_from_slice(&session.leak_lpm);
        group
            .pressure_cmh2o
            .extend_from_slice(&session.pressure_cmh2o);
    }

    groups
        .into_iter()
        .map(|(day, group)| {
            let usage = summarize_therapy_usage(&group.usage)?;
            let event_indices = if usage.therapy_ms > 0 {
                Some(calculate_event_indices(group.events, usage.therapy_ms)?)
            } else if group.events.is_zero() {
                None
            } else {
                return Err(AnalyticsError::NoTherapyTime);
            };
            let signals = summarize_leak_and_pressure(&group.leak_lpm, &group.pressure_cmh2o)?;
            Ok(DailySummary {
                day,
                usage,
                event_counts: group.events,
                event_indices,
                signals,
            })
        })
        .collect()
}

#[derive(Default)]
struct DailyAccumulator {
    usage: Vec<SessionUsageInput>,
    events: AhiEventCounts,
    leak_lpm: Vec<WeightedSample>,
    pressure_cmh2o: Vec<WeightedSample>,
}

fn is_leap_year(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

fn days_in_month(year: i32, month: u8) -> Option<u8> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if is_leap_year(year) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

// Howard Hinnant's civil calendar transform, expressed on i64 values so every
// i32 year is representable. The returned epoch is 1970-01-01.
fn days_from_civil(date: CivilDate) -> i64 {
    let mut year = i64::from(date.year);
    let month = i64::from(date.month);
    let day = i64::from(date.day);
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - UNIX_EPOCH_ADJUSTMENT_DAYS
}

fn civil_from_days(days: i64) -> Result<CivilDate, AnalyticsError> {
    // i128 intermediates make arbitrary public TherapyDay keys safe to inspect.
    let shifted = i128::from(days) + i128::from(UNIX_EPOCH_ADJUSTMENT_DAYS);
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i128::from(month <= 2);

    let year = i32::try_from(year)
        .map_err(|_| AnalyticsError::ArithmeticOverflow("converting therapy day to year"))?;
    let month = u8::try_from(month)
        .map_err(|_| AnalyticsError::ArithmeticOverflow("converting therapy day to month"))?;
    let day = u8::try_from(day)
        .map_err(|_| AnalyticsError::ArithmeticOverflow("converting therapy day to day"))?;
    CivilDate::new(year, month, day)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    fn local_ms(day: i64, hour: i64, minute: i64) -> i64 {
        day * MILLIS_PER_DAY + hour * 3_600_000 + minute * 60_000
    }

    fn corrected_local(value: i64) -> CorrectedLocalTimestampMs {
        CorrectedLocalTimestampMs::from_naive_local_epoch_ms(value)
    }

    #[test]
    fn noon_boundary_assigns_before_noon_to_previous_date() {
        assert_eq!(
            therapy_day_from_local_epoch_ms(corrected_local(local_ms(1, 11, 59)))
                .days_since_unix_epoch(),
            0
        );
        assert_eq!(
            therapy_day_from_local_epoch_ms(corrected_local(local_ms(1, 12, 0)))
                .days_since_unix_epoch(),
            1
        );
        assert_eq!(
            therapy_day_from_local_epoch_ms(corrected_local(-1)).days_since_unix_epoch(),
            -1
        );
        assert_eq!(
            therapy_day_from_local_epoch_ms(corrected_local(-NOON_MILLIS - 1))
                .days_since_unix_epoch(),
            -2
        );
    }

    #[test]
    fn civil_dates_round_trip_including_leap_days_and_pre_epoch_dates() {
        for date in [
            CivilDate::new(1970, 1, 1).expect("valid date"),
            CivilDate::new(1969, 12, 31).expect("valid date"),
            CivilDate::new(2000, 2, 29).expect("valid date"),
            CivilDate::new(2100, 3, 1).expect("valid date"),
            CivilDate::new(-44, 3, 15).expect("valid date"),
        ] {
            let key = TherapyDay::from_civil_date(date).expect("valid therapy day");
            assert_eq!(key.to_civil_date().expect("representable date"), date);
        }
        assert_eq!(
            TherapyDay::from_civil_date(CivilDate::new(1970, 1, 1).expect("valid date"))
                .expect("valid key")
                .days_since_unix_epoch(),
            0
        );
    }

    #[test]
    fn every_date_in_a_full_gregorian_cycle_round_trips() {
        // The Gregorian leap-year pattern repeats every 400 years. Cover one
        // full cycle on each side of the Unix epoch with no random generator.
        for year in 1600..=2399 {
            for month in 1..=12 {
                let last_day = days_in_month(year, month).expect("valid month");
                for day in 1..=last_day {
                    let date = CivilDate::new(year, month, day).expect("valid generated date");
                    let key = TherapyDay::from_civil_date(date).expect("valid therapy day");
                    assert_eq!(key.to_civil_date().expect("representable date"), date);
                }
            }
        }
    }

    #[test]
    fn invalid_dates_and_timestamp_overflow_are_rejected() {
        assert!(matches!(
            CivilDate::new(2023, 2, 29),
            Err(AnalyticsError::InvalidCivilDate { .. })
        ));
        assert!(matches!(
            CorrectedLocalTimestampMs::from_normalized_utc(i64::MAX, 1),
            Err(AnalyticsError::ArithmeticOverflow(_))
        ));
        assert!(matches!(
            CorrectedLocalTimestampMs::from_raw_device_local_epoch_ms(i64::MIN, -1),
            Err(AnalyticsError::ArithmeticOverflow(_))
        ));
    }

    #[test]
    fn normalized_utc_and_raw_device_paths_agree_when_correction_crosses_noon() {
        let raw_local = local_ms(2, 11, 59) + 59_500;
        let correction_ms = 1_000;
        let corrected_local = raw_local + correction_ms;
        let offset_seconds = 7 * 3_600;
        let normalized_utc = corrected_local - i64::from(offset_seconds) * 1_000;

        assert_eq!(
            therapy_day_from_raw_device_local_epoch_ms(raw_local, 0)
                .expect("raw time fits")
                .days_since_unix_epoch(),
            1
        );
        let from_raw = therapy_day_from_raw_device_local_epoch_ms(raw_local, correction_ms)
            .expect("correction fits");
        let from_normalized =
            therapy_day_from_normalized_utc(normalized_utc, offset_seconds).expect("offset fits");
        assert_eq!(from_raw.days_since_unix_epoch(), 2);
        assert_eq!(from_normalized, from_raw);
    }

    #[test]
    fn daily_aggregation_unions_usage_and_sums_events_and_signals() {
        let sessions = [
            SessionAnalyticsInput {
                enabled: true,
                local_start: corrected_local(local_ms(20_000, 22, 0)),
                usage: SessionUsageInput::new(0, 7_200_000, vec![]),
                events: AhiEventCounts {
                    obstructive_apnea: 1,
                    ..AhiEventCounts::default()
                },
                leak_lpm: vec![WeightedSample::new(5.0, 1_000)],
                pressure_cmh2o: vec![WeightedSample::new(8.0, 1_000)],
            },
            SessionAnalyticsInput {
                enabled: true,
                local_start: corrected_local(local_ms(20_001, 3, 0)),
                usage: SessionUsageInput::new(3_600_000, 10_800_000, vec![]),
                events: AhiEventCounts {
                    hypopnea: 2,
                    ..AhiEventCounts::default()
                },
                leak_lpm: vec![WeightedSample::new(7.0, 1_000)],
                pressure_cmh2o: vec![WeightedSample::new(10.0, 1_000)],
            },
            SessionAnalyticsInput {
                enabled: false,
                local_start: corrected_local(local_ms(20_005, 22, 0)),
                usage: SessionUsageInput::new(0, 3_600_000, vec![]),
                events: AhiEventCounts {
                    clear_airway: 100,
                    ..AhiEventCounts::default()
                },
                leak_lpm: vec![],
                pressure_cmh2o: vec![],
            },
        ];

        let days = aggregate_by_noon(&sessions).expect("valid daily aggregation");
        assert_eq!(days.len(), 1);
        let day = days[0];
        assert_eq!(day.day.days_since_unix_epoch(), 20_000);
        assert_eq!(day.usage.therapy_ms, 10_800_000);
        assert_eq!(day.event_counts.ahi_total().expect("counts fit"), 3);
        assert_eq!(day.event_indices.expect("usage exists").ahi, 1.0);
        assert_eq!(day.signals.leak_lpm.expect("leak exists").median, 6.0);
        assert_eq!(
            day.signals.pressure_cmh2o.expect("pressure exists").median,
            9.0
        );
    }

    #[test]
    fn output_days_are_sorted_and_empty_input_is_empty() {
        assert!(
            aggregate_by_noon(&[])
                .expect("empty input is valid")
                .is_empty()
        );

        let make = |day| SessionAnalyticsInput {
            enabled: true,
            local_start: corrected_local(local_ms(day, 13, 0)),
            usage: SessionUsageInput::new(day, day + 1, vec![]),
            events: AhiEventCounts::default(),
            leak_lpm: vec![],
            pressure_cmh2o: vec![],
        };
        let summaries =
            aggregate_by_noon(&[make(3), make(1), make(2)]).expect("valid daily aggregation");
        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.day.days_since_unix_epoch())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn events_without_therapy_are_rejected() {
        let session = SessionAnalyticsInput {
            enabled: true,
            local_start: corrected_local(local_ms(1, 13, 0)),
            usage: SessionUsageInput::new(0, 0, vec![]),
            events: AhiEventCounts {
                hypopnea: 1,
                ..AhiEventCounts::default()
            },
            leak_lpm: vec![],
            pressure_cmh2o: vec![],
        };
        assert_eq!(
            aggregate_by_noon(&[session]),
            Err(AnalyticsError::NoTherapyTime)
        );
    }
}
