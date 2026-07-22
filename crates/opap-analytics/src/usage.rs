use crate::{AnalyticsError, u64_as_f64};

/// Whether a session slice represents active therapy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceState {
    MaskOn,
    MaskOff,
}

/// A half-open session interval `[start_ms, end_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TherapySlice {
    pub start_ms: i64,
    pub end_ms: i64,
    pub state: SliceState,
}

impl TherapySlice {
    #[must_use]
    pub const fn mask_on(start_ms: i64, end_ms: i64) -> Self {
        Self {
            start_ms,
            end_ms,
            state: SliceState::MaskOn,
        }
    }

    #[must_use]
    pub const fn mask_off(start_ms: i64, end_ms: i64) -> Self {
        Self {
            start_ms,
            end_ms,
            state: SliceState::MaskOff,
        }
    }
}

/// One session window and its optional mask-on/mask-off slices.
///
/// With no slices, the complete session window represents therapy. When one or
/// more slices exist, only `MaskOn` slices contribute. This mirrors OSCAR's
/// `Session::hours()` behavior at the pinned reference revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUsageInput {
    pub start_ms: i64,
    pub end_ms: i64,
    pub slices: Vec<TherapySlice>,
}

impl SessionUsageInput {
    #[must_use]
    pub const fn new(start_ms: i64, end_ms: i64, slices: Vec<TherapySlice>) -> Self {
        Self {
            start_ms,
            end_ms,
            slices,
        }
    }
}

/// Unioned therapy duration and input accounting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsageSummary {
    pub therapy_ms: u64,
    pub session_count: usize,
    pub supplied_slice_count: usize,
    pub mask_on_interval_count: usize,
    pub merged_interval_count: usize,
}

impl UsageSummary {
    /// Therapy duration in decimal hours.
    #[must_use]
    pub fn hours(self) -> f64 {
        u64_as_f64(self.therapy_ms) / 3_600_000.0
    }
}

/// Summarize a single session, applying the full-window fallback when it has no slices.
///
/// # Errors
///
/// Returns an error for reversed, overlapping, or out-of-window intervals, or
/// duration/count overflow.
pub fn summarize_session_usage(
    session: &SessionUsageInput,
) -> Result<UsageSummary, AnalyticsError> {
    summarize_therapy_usage(core::slice::from_ref(session))
}

/// Union active therapy across sessions so overlapping time is counted once.
///
/// OSCAR performs an equivalent interval union for a day's enabled sessions;
/// see `OSCAR_PROVENANCE.md`. Adjacent half-open intervals are merged as an
/// implementation detail but retain the same duration.
///
/// # Errors
///
/// Returns an error for reversed, overlapping, or out-of-window intervals, or
/// duration/count overflow.
pub fn summarize_therapy_usage(
    sessions: &[SessionUsageInput],
) -> Result<UsageSummary, AnalyticsError> {
    let mut intervals = Vec::new();
    let mut supplied_slice_count = 0_usize;
    let mut mask_on_interval_count = 0_usize;

    for session in sessions {
        validate_interval(session.start_ms, session.end_ms)?;
        supplied_slice_count = supplied_slice_count
            .checked_add(session.slices.len())
            .ok_or(AnalyticsError::ArithmeticOverflow(
                "counting therapy slices",
            ))?;

        if session.slices.is_empty() {
            if session.start_ms != session.end_ms {
                intervals.push((session.start_ms, session.end_ms));
                mask_on_interval_count = mask_on_interval_count.checked_add(1).ok_or(
                    AnalyticsError::ArithmeticOverflow("counting mask-on intervals"),
                )?;
            }
            continue;
        }

        let mut ordered_slices = Vec::with_capacity(session.slices.len());
        for slice in &session.slices {
            validate_interval(slice.start_ms, slice.end_ms)?;
            if slice.start_ms < session.start_ms || slice.end_ms > session.end_ms {
                return Err(AnalyticsError::SliceOutsideSession {
                    session_start_ms: session.start_ms,
                    session_end_ms: session.end_ms,
                    slice_start_ms: slice.start_ms,
                    slice_end_ms: slice.end_ms,
                });
            }
            if slice.start_ms != slice.end_ms {
                ordered_slices.push(*slice);
            }
        }
        ordered_slices.sort_unstable_by_key(|slice| (slice.start_ms, slice.end_ms));

        for pair in ordered_slices.windows(2) {
            let first = pair[0];
            let second = pair[1];
            if second.start_ms < first.end_ms {
                return Err(AnalyticsError::OverlappingSessionSlices {
                    first_start_ms: first.start_ms,
                    first_end_ms: first.end_ms,
                    second_start_ms: second.start_ms,
                    second_end_ms: second.end_ms,
                });
            }
        }

        for slice in ordered_slices {
            if slice.state == SliceState::MaskOn && slice.start_ms != slice.end_ms {
                intervals.push((slice.start_ms, slice.end_ms));
                mask_on_interval_count = mask_on_interval_count.checked_add(1).ok_or(
                    AnalyticsError::ArithmeticOverflow("counting mask-on intervals"),
                )?;
            }
        }
    }

    intervals.sort_unstable_by_key(|&(start, end)| (start, end));

    let mut therapy_ms = 0_u64;
    let mut merged_interval_count = 0_usize;
    let mut active: Option<(i64, i64)> = None;

    for (start, end) in intervals {
        match active {
            None => active = Some((start, end)),
            Some((active_start, active_end)) if start <= active_end => {
                active = Some((active_start, active_end.max(end)));
            }
            Some(previous) => {
                therapy_ms = add_interval_duration(therapy_ms, previous)?;
                merged_interval_count = merged_interval_count.checked_add(1).ok_or(
                    AnalyticsError::ArithmeticOverflow("counting merged therapy intervals"),
                )?;
                active = Some((start, end));
            }
        }
    }

    if let Some(previous) = active {
        therapy_ms = add_interval_duration(therapy_ms, previous)?;
        merged_interval_count =
            merged_interval_count
                .checked_add(1)
                .ok_or(AnalyticsError::ArithmeticOverflow(
                    "counting merged therapy intervals",
                ))?;
    }

    Ok(UsageSummary {
        therapy_ms,
        session_count: sessions.len(),
        supplied_slice_count,
        mask_on_interval_count,
        merged_interval_count,
    })
}

fn validate_interval(start_ms: i64, end_ms: i64) -> Result<(), AnalyticsError> {
    if end_ms < start_ms {
        return Err(AnalyticsError::InvalidInterval { start_ms, end_ms });
    }
    Ok(())
}

fn add_interval_duration(total: u64, interval: (i64, i64)) -> Result<u64, AnalyticsError> {
    let duration = i128::from(interval.1) - i128::from(interval.0);
    let duration = u64::try_from(duration)
        .map_err(|_| AnalyticsError::ArithmeticOverflow("representing therapy duration"))?;
    total
        .checked_add(duration)
        .ok_or(AnalyticsError::ArithmeticOverflow(
            "summing therapy duration",
        ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn full_window_is_used_when_slices_are_absent() {
        let usage = summarize_session_usage(&SessionUsageInput::new(1_000, 3_500, vec![]))
            .expect("valid usage");
        assert_eq!(usage.therapy_ms, 2_500);
        assert_eq!(usage.mask_on_interval_count, 1);
    }

    #[test]
    fn only_mask_on_slices_contribute() {
        let session = SessionUsageInput::new(
            0,
            10_000,
            vec![
                TherapySlice::mask_on(0, 3_000),
                TherapySlice::mask_off(3_000, 5_000),
                TherapySlice::mask_on(5_000, 7_000),
            ],
        );
        let usage = summarize_session_usage(&session).expect("valid usage");
        assert_eq!(usage.therapy_ms, 5_000);
        assert_eq!(usage.mask_on_interval_count, 2);
        assert_eq!(usage.merged_interval_count, 2);
    }

    #[test]
    fn overlapping_sessions_are_not_double_counted() {
        let sessions = [
            SessionUsageInput::new(0, 10_000, vec![]),
            SessionUsageInput::new(5_000, 15_000, vec![]),
        ];
        let usage = summarize_therapy_usage(&sessions).expect("valid usage");
        assert_eq!(usage.therapy_ms, 15_000);
    }

    #[test]
    fn union_is_order_invariant_for_deterministic_permutations() {
        let source = [
            SessionUsageInput::new(10, 20, vec![]),
            SessionUsageInput::new(-5, 5, vec![]),
            SessionUsageInput::new(3, 12, vec![]),
            SessionUsageInput::new(30, 40, vec![]),
        ];
        let expected = summarize_therapy_usage(&source)
            .expect("valid usage")
            .therapy_ms;

        for shift in 0..source.len() {
            let rotated = (0..source.len())
                .map(|index| source[(index + shift) % source.len()].clone())
                .collect::<Vec<_>>();
            assert_eq!(
                summarize_therapy_usage(&rotated)
                    .expect("valid usage")
                    .therapy_ms,
                expected
            );
        }

        let mut reversed = source.to_vec();
        reversed.reverse();
        assert_eq!(
            summarize_therapy_usage(&reversed)
                .expect("valid usage")
                .therapy_ms,
            expected
        );
    }

    #[test]
    fn union_matches_discrete_coverage_for_all_small_interval_pairs() {
        for first_start in 0_i64..=5 {
            for first_end in first_start..=5 {
                for second_start in 0_i64..=5 {
                    for second_end in second_start..=5 {
                        let sessions = [
                            SessionUsageInput::new(first_start, first_end, vec![]),
                            SessionUsageInput::new(second_start, second_end, vec![]),
                        ];
                        let actual = summarize_therapy_usage(&sessions)
                            .expect("ordered small intervals")
                            .therapy_ms;
                        let expected = (0_i64..5)
                            .filter(|point| {
                                (first_start <= *point && *point < first_end)
                                    || (second_start <= *point && *point < second_end)
                            })
                            .count();
                        assert_eq!(actual, u64::try_from(expected).expect("small count fits"));
                    }
                }
            }
        }
    }

    #[test]
    fn extreme_timestamp_span_is_computed_without_signed_overflow() {
        let usage = summarize_session_usage(&SessionUsageInput::new(i64::MIN, i64::MAX, vec![]))
            .expect("the full i64 timestamp span fits in u64");
        assert_eq!(usage.therapy_ms, u64::MAX);
        assert!(usage.hours().is_finite());
    }

    #[test]
    fn invalid_intervals_and_out_of_window_slices_are_rejected() {
        assert!(matches!(
            summarize_session_usage(&SessionUsageInput::new(2, 1, vec![])),
            Err(AnalyticsError::InvalidInterval { .. })
        ));
        assert!(matches!(
            summarize_session_usage(&SessionUsageInput::new(
                0,
                10,
                vec![TherapySlice::mask_on(9, 11)]
            )),
            Err(AnalyticsError::SliceOutsideSession { .. })
        ));
        assert!(matches!(
            summarize_session_usage(&SessionUsageInput::new(
                0,
                10,
                vec![TherapySlice::mask_on(0, 6), TherapySlice::mask_off(5, 8),]
            )),
            Err(AnalyticsError::OverlappingSessionSlices { .. })
        ));
    }
}
