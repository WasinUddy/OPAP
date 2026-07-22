use core::cmp::Ordering;

use crate::{AnalyticsError, u64_as_f64};

/// One physical signal value and the integer milliseconds it represents.
///
/// For a regularly sampled signal, every sample must use the same period. For
/// step/event data, use the time until the next value. Integer millisecond
/// weights can represent OSCAR's pinned single-waveform-list `count * rate`
/// convention when an importer supplies the same timing. Across multiple
/// waveform lists, the pinned source re-adds all cumulative value counts using
/// each current list's rate. OPAP deliberately makes duration explicit and
/// does not reproduce that behavior or OSCAR's event-time truncation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeightedSample {
    pub value: f64,
    pub duration_ms: u64,
}

impl WeightedSample {
    #[must_use]
    pub const fn new(value: f64, duration_ms: u64) -> Self {
        Self { value, duration_ms }
    }
}

/// Descriptive statistics for one sampled channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalSummary {
    pub sample_count: usize,
    pub represented_ms: u64,
    pub min: f64,
    pub max: f64,
    pub weighted_mean: f64,
    pub median: f64,
    pub p90: f64,
    pub p95: f64,
    pub p995: f64,
}

/// Named summaries for the two therapy signals shown most often by the UI.
///
/// Inputs are physical units: litres/minute for leak and cmH2O for pressure.
/// Empty channels produce `None`; malformed non-empty channels return an error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeakPressureSummary {
    pub leak_lpm: Option<SignalSummary>,
    pub pressure_cmh2o: Option<SignalSummary>,
}

/// Summarize a signal using guarded, time-weighted day-style percentile ranks.
///
/// # Errors
///
/// Returns an error for empty/non-finite/zero-duration input or arithmetic overflow.
pub fn summarize_signal(samples: &[WeightedSample]) -> Result<SignalSummary, AnalyticsError> {
    let prepared = PreparedSignal::new(samples)?;
    let Some(first) = prepared.bins.first() else {
        return Err(AnalyticsError::EmptySignal);
    };
    let Some(last) = prepared.bins.last() else {
        return Err(AnalyticsError::EmptySignal);
    };
    Ok(SignalSummary {
        sample_count: samples.len(),
        represented_ms: prepared.total_weight,
        min: first.value,
        max: last.value,
        weighted_mean: prepared.weighted_mean()?,
        median: prepared.percentile(0.50)?,
        p90: prepared.percentile(0.90)?,
        p95: prepared.percentile(0.95)?,
        p995: prepared.percentile(0.995)?,
    })
}

/// Summarize a regular signal where each value represents `sample_period_ms`.
///
/// A parser should derive this integer period from the source format's record
/// duration and samples-per-record. If the true period is fractional, callers
/// should construct `WeightedSample`s with an explicit millisecond allocation
/// policy (for example, distributing remainder milliseconds across a record).
///
/// # Errors
///
/// Returns an error for empty/non-finite input, a zero period, or duration overflow.
pub fn summarize_regular_signal(
    values: &[f64],
    sample_period_ms: u64,
) -> Result<SignalSummary, AnalyticsError> {
    if sample_period_ms == 0 {
        return Err(AnalyticsError::ZeroSamplePeriod);
    }
    if values.is_empty() {
        return Err(AnalyticsError::EmptySignal);
    }

    let count = u64::try_from(values.len())
        .map_err(|_| AnalyticsError::ArithmeticOverflow("converting signal sample count"))?;
    count
        .checked_mul(sample_period_ms)
        .ok_or(AnalyticsError::ArithmeticOverflow(
            "calculating regular signal duration",
        ))?;

    let samples = values
        .iter()
        .copied()
        .map(|value| WeightedSample::new(value, sample_period_ms))
        .collect::<Vec<_>>();
    summarize_signal(&samples)
}

/// Calculate one weighted percentile in the inclusive range `0.0..=1.0`.
///
/// This is derived from OSCAR's pinned `Day::percentile` time-weighted rank
/// walk. At an exact cumulative-weight boundary it interpolates between
/// adjacent value-bin midpoints. The pinned C++ can extrapolate beyond the next
/// observed bin and can address one element past the final bin; OPAP returns
/// the final value when there is no successor and clamps the interpolation to
/// preserve `min <= result <= max`. It also rejects invalid inputs. This is not
/// OSCAR `Session::percentile`, which is an unweighted `nth_element` operation,
/// and no parity is claimed. See `OSCAR_PROVENANCE.md` for exact source links.
///
/// # Errors
///
/// Returns an error for an invalid percentile, malformed samples, or arithmetic overflow.
pub fn weighted_percentile(
    samples: &[WeightedSample],
    percentile: f64,
) -> Result<f64, AnalyticsError> {
    if !percentile.is_finite() || !(0.0..=1.0).contains(&percentile) {
        return Err(AnalyticsError::InvalidPercentile);
    }
    PreparedSignal::new(samples)?.percentile(percentile)
}

/// Summarize leak and pressure channels without conflating their units.
///
/// # Errors
///
/// Returns an error if either non-empty channel is malformed or overflows.
pub fn summarize_leak_and_pressure(
    leak_lpm: &[WeightedSample],
    pressure_cmh2o: &[WeightedSample],
) -> Result<LeakPressureSummary, AnalyticsError> {
    Ok(LeakPressureSummary {
        leak_lpm: optional_summary(leak_lpm)?,
        pressure_cmh2o: optional_summary(pressure_cmh2o)?,
    })
}

fn optional_summary(samples: &[WeightedSample]) -> Result<Option<SignalSummary>, AnalyticsError> {
    if samples.is_empty() {
        Ok(None)
    } else {
        summarize_signal(samples).map(Some)
    }
}

#[derive(Debug, Clone, Copy)]
struct ValueBin {
    value: f64,
    weight: u64,
}

struct PreparedSignal {
    bins: Vec<ValueBin>,
    total_weight: u64,
}

impl PreparedSignal {
    fn new(samples: &[WeightedSample]) -> Result<Self, AnalyticsError> {
        if samples.is_empty() {
            return Err(AnalyticsError::EmptySignal);
        }

        let mut total_weight = 0_u64;
        let mut bins = Vec::with_capacity(samples.len());
        for (index, sample) in samples.iter().enumerate() {
            if !sample.value.is_finite() {
                return Err(AnalyticsError::NonFiniteSignalValue { index });
            }
            if sample.duration_ms == 0 {
                return Err(AnalyticsError::ZeroSampleWeight { index });
            }
            total_weight = total_weight.checked_add(sample.duration_ms).ok_or(
                AnalyticsError::ArithmeticOverflow("summing signal sample durations"),
            )?;
            bins.push(ValueBin {
                value: sample.value,
                weight: sample.duration_ms,
            });
        }

        bins.sort_unstable_by(|left, right| left.value.total_cmp(&right.value));

        let mut merged: Vec<ValueBin> = Vec::with_capacity(bins.len());
        for bin in bins {
            if let Some(previous) = merged.last_mut() {
                if previous.value.partial_cmp(&bin.value) == Some(Ordering::Equal) {
                    previous.weight = previous.weight.checked_add(bin.weight).ok_or(
                        AnalyticsError::ArithmeticOverflow("merging equal signal values"),
                    )?;
                    continue;
                }
            }
            merged.push(bin);
        }

        Ok(Self {
            bins: merged,
            total_weight,
        })
    }

    fn weighted_mean(&self) -> Result<f64, AnalyticsError> {
        let total = u64_as_f64(self.total_weight);
        let mut mean = 0.0_f64;
        for bin in &self.bins {
            let fraction = u64_as_f64(bin.weight) / total;
            let term = bin.value * fraction;
            if !term.is_finite() {
                return Err(AnalyticsError::ArithmeticOverflow(
                    "calculating weighted signal mean",
                ));
            }
            mean += term;
            if !mean.is_finite() {
                return Err(AnalyticsError::ArithmeticOverflow(
                    "summing weighted signal mean",
                ));
            }
        }
        Ok(mean)
    }

    fn percentile(&self, percentile: f64) -> Result<f64, AnalyticsError> {
        debug_assert!(percentile.is_finite() && (0.0..=1.0).contains(&percentile));

        let target_floor = floor_weight_rank(self.total_weight, percentile);
        let mut cumulative = 0_u64;

        for (index, bin) in self.bins.iter().enumerate() {
            cumulative =
                cumulative
                    .checked_add(bin.weight)
                    .ok_or(AnalyticsError::ArithmeticOverflow(
                        "walking percentile weights",
                    ))?;
            if cumulative > target_floor {
                return Ok(bin.value);
            }

            if cumulative == target_floor {
                let Some(next) = self.bins.get(index + 1) else {
                    return Ok(bin.value);
                };

                let total = u64_as_f64(self.total_weight);
                let cumulative_f64 = u64_as_f64(cumulative);
                let lower_rank = (cumulative_f64 - (u64_as_f64(bin.weight) / 2.0)) / total;
                let upper_rank = (cumulative_f64 + (u64_as_f64(next.weight) / 2.0)) / total;
                let span = upper_rank - lower_rank;
                if !span.is_finite() || span <= 0.0 {
                    return Err(AnalyticsError::ArithmeticOverflow(
                        "interpolating weighted percentile",
                    ));
                }

                let factor = ((percentile - lower_rank) / span).clamp(0.0, 1.0);
                let value = bin.value * (1.0 - factor) + next.value * factor;
                if !value.is_finite() {
                    return Err(AnalyticsError::ArithmeticOverflow(
                        "interpolating weighted percentile value",
                    ));
                }
                return Ok(value.clamp(bin.value, next.value));
            }
        }

        Ok(self.bins.last().expect("validated non-empty bins").value)
    }
}

/// Percentile input is validated and the product is in `0..=u64::MAX`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn floor_weight_rank(total_weight: u64, percentile: f64) -> u64 {
    (u64_as_f64(total_weight) * percentile).floor() as u64
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    fn equal_weight(values: &[f64]) -> Vec<WeightedSample> {
        values
            .iter()
            .copied()
            .map(|value| WeightedSample::new(value, 1))
            .collect()
    }

    #[test]
    fn regular_signal_has_explicit_duration_and_expected_summary() {
        let summary =
            summarize_regular_signal(&[1.0, 2.0, 3.0, 4.0], 40).expect("finite regular signal");
        assert_eq!(summary.sample_count, 4);
        assert_eq!(summary.represented_ms, 160);
        assert_eq!(summary.min, 1.0);
        assert_eq!(summary.max, 4.0);
        assert_eq!(summary.weighted_mean, 2.5);
        assert_eq!(summary.median, 2.5);
        assert_eq!(summary.p95, 4.0);
    }

    #[test]
    fn duration_weighting_differs_from_unweighted_counting() {
        let samples = [WeightedSample::new(1.0, 3), WeightedSample::new(10.0, 1)];
        let summary = summarize_signal(&samples).expect("valid weighted signal");
        assert_eq!(summary.weighted_mean, 3.25);
        assert_eq!(summary.median, 1.0);
    }

    #[test]
    fn percentile_endpoints_and_upper_tail_are_bounded() {
        let samples = equal_weight(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(weighted_percentile(&samples, 0.0).expect("valid p0"), 1.0);
        assert_eq!(weighted_percentile(&samples, 1.0).expect("valid p1"), 4.0);
        assert_eq!(weighted_percentile(&samples, 0.95).expect("valid p95"), 4.0);
    }

    #[test]
    fn percentiles_are_monotonic_for_many_deterministic_ranks() {
        let samples = [
            WeightedSample::new(-4.0, 7),
            WeightedSample::new(1.5, 2),
            WeightedSample::new(9.0, 11),
            WeightedSample::new(3.0, 5),
        ];
        let mut previous = f64::NEG_INFINITY;
        for step in 0_u32..=100 {
            let percentile = f64::from(step) / 100.0;
            let current = weighted_percentile(&samples, percentile).expect("valid percentile");
            assert!(current >= previous, "p={percentile} regressed");
            assert!((-4.0..=9.0).contains(&current));
            previous = current;
        }
    }

    #[test]
    fn summary_is_invariant_to_input_order() {
        let source = [
            WeightedSample::new(3.0, 5),
            WeightedSample::new(-1.0, 2),
            WeightedSample::new(7.0, 3),
            WeightedSample::new(3.0, 4),
        ];
        let expected = summarize_signal(&source).expect("valid summary");

        for shift in 0..source.len() {
            let rotated = (0..source.len())
                .map(|index| source[(index + shift) % source.len()])
                .collect::<Vec<_>>();
            assert_eq!(summarize_signal(&rotated).expect("valid summary"), expected);
        }
    }

    #[test]
    fn invalid_signal_inputs_are_rejected() {
        assert_eq!(summarize_signal(&[]), Err(AnalyticsError::EmptySignal));
        assert_eq!(
            summarize_regular_signal(&[1.0], 0),
            Err(AnalyticsError::ZeroSamplePeriod)
        );
        assert!(matches!(
            summarize_signal(&[WeightedSample::new(f64::NAN, 1)]),
            Err(AnalyticsError::NonFiniteSignalValue { index: 0 })
        ));
        assert!(matches!(
            summarize_signal(&[WeightedSample::new(1.0, 0)]),
            Err(AnalyticsError::ZeroSampleWeight { index: 0 })
        ));
        assert_eq!(
            weighted_percentile(&[WeightedSample::new(1.0, 1)], f64::INFINITY),
            Err(AnalyticsError::InvalidPercentile)
        );
    }

    #[test]
    fn weight_overflow_is_rejected() {
        let samples = [
            WeightedSample::new(1.0, u64::MAX),
            WeightedSample::new(2.0, 1),
        ];
        assert!(matches!(
            summarize_signal(&samples),
            Err(AnalyticsError::ArithmeticOverflow(_))
        ));
    }

    #[test]
    fn leak_and_pressure_remain_separate_and_empty_is_absent() {
        let pressure = [WeightedSample::new(8.0, 1_000)];
        let summary = summarize_leak_and_pressure(&[], &pressure).expect("valid channels");
        assert_eq!(summary.leak_lpm, None);
        assert_eq!(
            summary.pressure_cmh2o.expect("pressure present").median,
            8.0
        );
    }
}
