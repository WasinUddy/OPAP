use crate::{AnalyticsError, u64_as_f64};

/// Device event counts used by OSCAR's AHI and RDI formulas.
///
/// The five AHI fields correspond to the pinned OSCAR `ahiChannels` list. They
/// are kept distinct because different device loaders populate different
/// channels. `rera` contributes to RDI, not AHI.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AhiEventCounts {
    pub clear_airway: u64,
    pub obstructive_apnea: u64,
    pub hypopnea: u64,
    pub unclassified_apnea: u64,
    pub device_reported_apnea: u64,
    pub rera: u64,
}

impl AhiEventCounts {
    /// Sum the five OSCAR AHI-contributing channels with checked arithmetic.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError::ArithmeticOverflow`] if the count sum exceeds `u64`.
    pub fn ahi_total(self) -> Result<u64, AnalyticsError> {
        [
            self.clear_airway,
            self.obstructive_apnea,
            self.hypopnea,
            self.unclassified_apnea,
            self.device_reported_apnea,
        ]
        .into_iter()
        .try_fold(0_u64, |total, count| {
            total
                .checked_add(count)
                .ok_or(AnalyticsError::ArithmeticOverflow(
                    "summing AHI event counts",
                ))
        })
    }

    /// Sum AHI events and RERA events with checked arithmetic.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError::ArithmeticOverflow`] if the count sum exceeds `u64`.
    pub fn rdi_total(self) -> Result<u64, AnalyticsError> {
        self.ahi_total()?
            .checked_add(self.rera)
            .ok_or(AnalyticsError::ArithmeticOverflow(
                "summing RDI event counts",
            ))
    }

    pub(crate) fn checked_add(self, other: Self) -> Result<Self, AnalyticsError> {
        Ok(Self {
            clear_airway: add_count(self.clear_airway, other.clear_airway)?,
            obstructive_apnea: add_count(self.obstructive_apnea, other.obstructive_apnea)?,
            hypopnea: add_count(self.hypopnea, other.hypopnea)?,
            unclassified_apnea: add_count(self.unclassified_apnea, other.unclassified_apnea)?,
            device_reported_apnea: add_count(
                self.device_reported_apnea,
                other.device_reported_apnea,
            )?,
            rera: add_count(self.rera, other.rera)?,
        })
    }

    pub(crate) fn is_zero(self) -> bool {
        self == Self::default()
    }
}

/// Per-hour components computed against unioned active-therapy duration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventIndices {
    pub therapy_hours: f64,
    pub clear_airway_index: f64,
    pub obstructive_apnea_index: f64,
    pub hypopnea_index: f64,
    pub unclassified_apnea_index: f64,
    pub device_reported_apnea_index: f64,
    pub ahi: f64,
    pub rera_index: f64,
    pub rdi: f64,
}

/// Calculate event counts per therapy hour, including AHI and RDI.
///
/// This uses the formula derived from OSCAR's pinned `Day::calcAHI()` and
/// `Day::calcRDI()` source when positive `therapy_ms` and channel counts are
/// identical. It adds zero-time and overflow guards and does not claim
/// end-to-end parity or attach a clinical interpretation to the numbers.
///
/// # Errors
///
/// Returns an error when therapy time is zero, counts overflow, or a finite
/// floating-point index cannot be represented.
pub fn calculate_event_indices(
    counts: AhiEventCounts,
    therapy_ms: u64,
) -> Result<EventIndices, AnalyticsError> {
    if therapy_ms == 0 {
        return Err(AnalyticsError::NoTherapyTime);
    }

    let therapy_hours = u64_as_f64(therapy_ms) / 3_600_000.0;
    if !therapy_hours.is_finite() || therapy_hours <= 0.0 {
        return Err(AnalyticsError::ArithmeticOverflow(
            "converting therapy duration to hours",
        ));
    }

    let per_hour = |count: u64| -> Result<f64, AnalyticsError> {
        let value = u64_as_f64(count) / therapy_hours;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(AnalyticsError::ArithmeticOverflow(
                "calculating an event index",
            ))
        }
    };

    Ok(EventIndices {
        therapy_hours,
        clear_airway_index: per_hour(counts.clear_airway)?,
        obstructive_apnea_index: per_hour(counts.obstructive_apnea)?,
        hypopnea_index: per_hour(counts.hypopnea)?,
        unclassified_apnea_index: per_hour(counts.unclassified_apnea)?,
        device_reported_apnea_index: per_hour(counts.device_reported_apnea)?,
        ahi: per_hour(counts.ahi_total()?)?,
        rera_index: per_hour(counts.rera)?,
        rdi: per_hour(counts.rdi_total()?)?,
    })
}

fn add_count(left: u64, right: u64) -> Result<u64, AnalyticsError> {
    left.checked_add(right)
        .ok_or(AnalyticsError::ArithmeticOverflow(
            "aggregating event counts",
        ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn components_ahi_and_rdi_use_one_duration() {
        let counts = AhiEventCounts {
            clear_airway: 2,
            obstructive_apnea: 3,
            hypopnea: 4,
            unclassified_apnea: 1,
            device_reported_apnea: 0,
            rera: 2,
        };
        let indices = calculate_event_indices(counts, 5 * 3_600_000).expect("valid index");

        assert_eq!(counts.ahi_total().expect("valid count"), 10);
        assert_eq!(indices.clear_airway_index, 0.4);
        assert_eq!(indices.ahi, 2.0);
        assert_eq!(indices.rera_index, 0.4);
        assert_eq!(indices.rdi, 2.4);
    }

    #[test]
    fn zero_duration_and_count_overflow_are_rejected() {
        assert_eq!(
            calculate_event_indices(AhiEventCounts::default(), 0),
            Err(AnalyticsError::NoTherapyTime)
        );

        let overflowing = AhiEventCounts {
            clear_airway: u64::MAX,
            obstructive_apnea: 1,
            ..AhiEventCounts::default()
        };
        assert!(matches!(
            overflowing.ahi_total(),
            Err(AnalyticsError::ArithmeticOverflow(_))
        ));
    }

    #[test]
    fn checked_aggregation_covers_every_component() {
        let one = AhiEventCounts {
            clear_airway: 1,
            obstructive_apnea: 2,
            hypopnea: 3,
            unclassified_apnea: 4,
            device_reported_apnea: 5,
            rera: 6,
        };
        let sum = one.checked_add(one).expect("counts fit");
        assert_eq!(
            sum,
            AhiEventCounts {
                clear_airway: 2,
                obstructive_apnea: 4,
                hypopnea: 6,
                unclassified_apnea: 8,
                device_reported_apnea: 10,
                rera: 12,
            }
        );
    }

    #[test]
    fn all_representable_indices_are_finite() {
        let indices = calculate_event_indices(
            AhiEventCounts {
                clear_airway: u64::MAX,
                ..AhiEventCounts::default()
            },
            1,
        )
        .expect("u64 converted to f64 remains finite");
        assert!(indices.ahi.is_finite());
    }
}
