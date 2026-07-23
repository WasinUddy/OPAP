// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (c) 2026 OPAP contributors
// Pinned OSCAR/SleepyHead provenance and differences are in README.md.

use crate::CalibrationError;

/// A validated EDF start date and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdfDateTime {
    pub year: u16,
    /// The exact two-digit year stored in the EDF header.
    pub year_two_digits: u8,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// The fixed EDF header plus all column-major signal descriptors.
#[derive(Debug, Clone, PartialEq)]
pub struct EdfHeader {
    pub version: String,
    pub patient_id: String,
    pub recording_id: String,
    pub start: EdfDateTime,
    pub header_bytes: usize,
    pub reserved: String,
    /// `None` represents the EDF sentinel `-1` (unknown until end of input).
    pub declared_record_count: Option<usize>,
    pub record_duration_seconds: f64,
    pub signals: Vec<SignalHeader>,
}

impl EdfHeader {
    /// True when the EDF reserved field declares continuous EDF+ data.
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        self.reserved.starts_with("EDF+C")
    }

    /// True when the EDF reserved field declares discontinuous EDF+ data.
    #[must_use]
    pub fn is_discontinuous(&self) -> bool {
        self.reserved.starts_with("EDF+D")
    }

    pub(crate) fn is_edf_plus(&self) -> bool {
        self.is_continuous() || self.is_discontinuous()
    }
}

/// Metadata and calibration for one signal.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalHeader {
    pub label: String,
    pub transducer_type: String,
    pub physical_dimension: String,
    pub physical_minimum: f64,
    pub physical_maximum: f64,
    pub digital_minimum: i32,
    pub digital_maximum: i32,
    pub prefiltering: String,
    pub samples_per_record: usize,
    pub reserved: String,
}

impl SignalHeader {
    pub(crate) fn empty() -> Self {
        Self {
            label: String::new(),
            transducer_type: String::new(),
            physical_dimension: String::new(),
            physical_minimum: 0.0,
            physical_maximum: 0.0,
            digital_minimum: 0,
            digital_maximum: 0,
            prefiltering: String::new(),
            samples_per_record: 0,
            reserved: String::new(),
        }
    }

    /// True under the case-sensitive rule verified in the pinned OSCAR source.
    ///
    /// The pinned parser recognizes any label containing `Annotations`; strict
    /// EDF+ uses the exact label `EDF Annotations`.
    #[must_use]
    pub fn is_annotation_signal(&self) -> bool {
        self.label.contains("Annotations")
    }

    /// True only for the annotation signal label required by EDF+.
    #[must_use]
    pub fn is_standard_annotation_signal(&self) -> bool {
        self.label == "EDF Annotations"
    }

    /// Multiplicative part of the EDF affine calibration.
    ///
    /// # Errors
    ///
    /// Returns an error for equal digital bounds or a non-finite result.
    pub fn gain(&self) -> Result<f64, CalibrationError> {
        let denominator = f64::from(self.digital_maximum) - f64::from(self.digital_minimum);
        if denominator == 0.0 {
            return Err(CalibrationError::EqualDigitalBounds);
        }
        let gain = (self.physical_maximum - self.physical_minimum) / denominator;
        if gain.is_finite() {
            Ok(gain)
        } else {
            Err(CalibrationError::NonFiniteResult)
        }
    }

    /// Additive part of the EDF affine calibration.
    ///
    /// # Errors
    ///
    /// Returns an error when [`Self::gain`] cannot be calculated.
    pub fn offset(&self) -> Result<f64, CalibrationError> {
        let offset = self.physical_minimum - f64::from(self.digital_minimum) * self.gain()?;
        if offset.is_finite() {
            Ok(offset)
        } else {
            Err(CalibrationError::NonFiniteResult)
        }
    }

    /// Convert a raw signed 16-bit sample to its physical value.
    ///
    /// # Errors
    ///
    /// Returns an error when the calibration is invalid.
    pub fn physical_value(&self, digital: i16) -> Result<f64, CalibrationError> {
        let value = f64::from(digital) * self.gain()? + self.offset()?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(CalibrationError::NonFiniteResult)
        }
    }
}

/// One EDF+ annotation text attached to an onset and optional duration.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub onset_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub text: String,
}

/// An annotation signal's decoded contents for one data record.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationRecord {
    pub record_index: usize,
    /// Onset from the record's empty EDF+ timekeeping TAL, when present.
    pub record_onset_seconds: Option<f64>,
    pub annotations: Vec<Annotation>,
}

/// Decoded values belonging to a signal.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalData {
    Digital(Vec<i16>),
    Annotations(Vec<AnnotationRecord>),
}

/// A signal descriptor paired with all of its decoded records.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub header: SignalHeader,
    pub data: SignalData,
}

impl Signal {
    #[must_use]
    pub fn digital_samples(&self) -> Option<&[i16]> {
        match &self.data {
            SignalData::Digital(samples) => Some(samples),
            SignalData::Annotations(_) => None,
        }
    }

    #[must_use]
    pub fn annotation_records(&self) -> Option<&[AnnotationRecord]> {
        match &self.data {
            SignalData::Digital(_) => None,
            SignalData::Annotations(records) => Some(records),
        }
    }

    /// Iterate over every sample converted to physical units.
    ///
    /// # Errors
    ///
    /// Returns an error for an annotation signal or invalid calibration.
    pub fn physical_samples(&self) -> Result<PhysicalSamples<'_>, CalibrationError> {
        let SignalData::Digital(samples) = &self.data else {
            return Err(CalibrationError::NotDigitalSignal);
        };
        let gain = self.header.gain()?;
        let offset = self.header.offset()?;
        for sample in [i16::MIN, i16::MAX] {
            if !(f64::from(sample) * gain + offset).is_finite() {
                return Err(CalibrationError::NonFiniteResult);
            }
        }
        Ok(PhysicalSamples {
            samples: samples.iter(),
            gain,
            offset,
        })
    }
}

/// Iterator over calibrated physical sample values.
#[derive(Debug, Clone)]
pub struct PhysicalSamples<'a> {
    samples: core::slice::Iter<'a, i16>,
    gain: f64,
    offset: f64,
}

impl Iterator for PhysicalSamples<'_> {
    type Item = f64;

    fn next(&mut self) -> Option<Self::Item> {
        self.samples
            .next()
            .map(|sample| f64::from(*sample) * self.gain + self.offset)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.samples.size_hint()
    }
}

impl ExactSizeIterator for PhysicalSamples<'_> {}
impl core::iter::FusedIterator for PhysicalSamples<'_> {}

/// A fully decoded EDF/EDF+ file.
#[derive(Debug, Clone, PartialEq)]
pub struct EdfFile {
    pub(crate) header: EdfHeader,
    pub(crate) signals: Vec<Signal>,
    pub(crate) record_count: usize,
    pub(crate) trailing_data_bytes: usize,
}

impl EdfFile {
    #[must_use]
    pub const fn header(&self) -> &EdfHeader {
        &self.header
    }

    #[must_use]
    pub fn signals(&self) -> &[Signal] {
        &self.signals
    }

    #[must_use]
    pub const fn record_count(&self) -> usize {
        self.record_count
    }

    /// Bytes after the declared records.
    ///
    /// Ignoring such bytes while decoding the declared count is a behavior
    /// verified in the OSCAR source pinned in this crate's README.
    #[must_use]
    pub const fn trailing_data_bytes(&self) -> usize {
        self.trailing_data_bytes
    }

    #[must_use]
    pub fn signal(&self, index: usize) -> Option<&Signal> {
        self.signals.get(index)
    }

    /// Return signals with exactly matching, case-sensitive labels.
    pub fn signals_named<'a>(&'a self, label: &'a str) -> impl Iterator<Item = &'a Signal> + 'a {
        self.signals
            .iter()
            .filter(move |signal| signal.header.label == label)
    }

    #[must_use]
    pub const fn records(&self) -> Records<'_> {
        Records {
            file: self,
            next_index: 0,
        }
    }

    #[must_use]
    pub fn record(&self, index: usize) -> Option<Record<'_>> {
        (index < self.record_count).then_some(Record { file: self, index })
    }
}

/// Iterator over data records while preserving signal order.
#[derive(Debug, Clone)]
pub struct Records<'a> {
    file: &'a EdfFile,
    next_index: usize,
}

impl<'a> Iterator for Records<'a> {
    type Item = Record<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.next_index;
        if index >= self.file.record_count {
            return None;
        }
        self.next_index += 1;
        Some(Record {
            file: self.file,
            index,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.file.record_count.saturating_sub(self.next_index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Records<'_> {}
impl core::iter::FusedIterator for Records<'_> {}

/// Borrowed view of one EDF data record.
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    file: &'a EdfFile,
    index: usize,
}

impl<'a> Record<'a> {
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    #[must_use]
    pub fn digital_samples(self, signal_index: usize) -> Option<&'a [i16]> {
        let signal = self.file.signals.get(signal_index)?;
        let SignalData::Digital(samples) = &signal.data else {
            return None;
        };
        let count = signal.header.samples_per_record;
        let start = self.index.checked_mul(count)?;
        let end = start.checked_add(count)?;
        samples.get(start..end)
    }

    #[must_use]
    pub fn annotations(self, signal_index: usize) -> Option<&'a [Annotation]> {
        self.annotation_record(signal_index)
            .map(|record| record.annotations.as_slice())
    }

    /// Return this record's decoded annotation entry for one signal.
    #[must_use]
    pub fn annotation_record(self, signal_index: usize) -> Option<&'a AnnotationRecord> {
        let signal = self.file.signals.get(signal_index)?;
        let SignalData::Annotations(records) = &signal.data else {
            return None;
        };
        records.get(self.index)
    }

    /// Return the first timekeeping TAL onset found across annotation signals.
    ///
    /// EDF+D uses this value to locate discontinuous records on the timeline.
    #[must_use]
    pub fn onset_seconds(self) -> Option<f64> {
        let standard = self
            .file
            .signals
            .iter()
            .find(|signal| signal.header.is_standard_annotation_signal());
        let compatible = self
            .file
            .signals
            .iter()
            .find(|signal| signal.header.is_annotation_signal());
        standard
            .or(compatible)
            .and_then(|signal| match &signal.data {
                SignalData::Annotations(records) => Some(records),
                SignalData::Digital(_) => None,
            })
            .and_then(|records| records.get(self.index))
            .and_then(|record| record.record_onset_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal_header(
        physical_minimum: f64,
        physical_maximum: f64,
        digital_minimum: i32,
        digital_maximum: i32,
    ) -> SignalHeader {
        SignalHeader {
            label: "Flow".to_owned(),
            transducer_type: String::new(),
            physical_dimension: "unit".to_owned(),
            physical_minimum,
            physical_maximum,
            digital_minimum,
            digital_maximum,
            prefiltering: String::new(),
            samples_per_record: 0,
            reserved: String::new(),
        }
    }

    #[test]
    fn complete_affine_calibration_maps_the_full_vector() {
        let header = signal_header(-17.5, 42.5, -2048, 1024);
        assert_eq!(header.gain(), Ok(0.019_531_25));
        assert_eq!(header.offset(), Ok(22.5));

        let digital = [-2048, -1024, 0, 512, 1024];
        let expected = [-17.5, 2.5, 22.5, 32.5, 42.5];
        for (sample, expected_value) in digital.into_iter().zip(expected) {
            assert_eq!(header.physical_value(sample), Ok(expected_value));
        }

        let signal = Signal {
            header,
            data: SignalData::Digital(digital.to_vec()),
        };
        assert_eq!(
            signal
                .physical_samples()
                .expect("finite affine calibration")
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn equal_digital_bounds_report_the_exact_error_category() {
        let header = signal_header(-1.0, 1.0, 7, 7);
        assert_eq!(header.gain(), Err(CalibrationError::EqualDigitalBounds));
        assert_eq!(header.offset(), Err(CalibrationError::EqualDigitalBounds));
        assert_eq!(
            header.physical_value(7),
            Err(CalibrationError::EqualDigitalBounds)
        );

        let signal = Signal {
            header,
            data: SignalData::Digital(vec![7]),
        };
        assert_eq!(
            signal
                .physical_samples()
                .expect_err("equal digital bounds cannot be calibrated"),
            CalibrationError::EqualDigitalBounds
        );
    }

    #[test]
    fn annotation_samples_report_not_digital_before_calibration() {
        let mut header = signal_header(-1.0, 1.0, 0, 0);
        header.label = "EDF Annotations".to_owned();
        let signal = Signal {
            header,
            data: SignalData::Annotations(Vec::new()),
        };

        assert_eq!(
            signal
                .physical_samples()
                .expect_err("annotations do not contain digital samples"),
            CalibrationError::NotDigitalSignal
        );
    }

    #[test]
    fn reversed_and_equal_physical_bounds_are_valid_calibrations() {
        let reversed = signal_header(30.0, -10.0, -2, 2);
        assert_eq!(reversed.gain(), Ok(-10.0));
        assert_eq!(reversed.offset(), Ok(10.0));
        let reversed_values =
            [-2, -1, 0, 1, 2].map(|sample| reversed.physical_value(sample).expect("finite value"));
        for (actual, expected) in reversed_values
            .into_iter()
            .zip([30.0_f64, 20.0, 10.0, 0.0, -10.0])
        {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }

        let equal = signal_header(7.5, 7.5, i32::from(i16::MIN), i32::from(i16::MAX));
        assert_eq!(equal.gain(), Ok(0.0));
        assert_eq!(equal.offset(), Ok(7.5));
        for sample in [i16::MIN, -1, 0, 1, i16::MAX] {
            assert_eq!(equal.physical_value(sample), Ok(7.5));
        }

        let signal = Signal {
            header: equal,
            data: SignalData::Digital(vec![i16::MIN, 0, i16::MAX]),
        };
        assert_eq!(
            signal
                .physical_samples()
                .expect("constant physical range is finite")
                .collect::<Vec<_>>(),
            [7.5, 7.5, 7.5]
        );
    }

    #[test]
    fn wrong_variant_accessors_return_none() {
        let mut annotation_header = signal_header(-1.0, 1.0, -1, 1);
        annotation_header.label = "EDF Annotations".to_owned();
        annotation_header.samples_per_record = 8;
        let annotation_record = AnnotationRecord {
            record_index: 0,
            record_onset_seconds: Some(0.0),
            annotations: vec![Annotation {
                onset_seconds: 0.5,
                duration_seconds: None,
                text: "event".to_owned(),
            }],
        };
        let annotation_signal = Signal {
            header: annotation_header.clone(),
            data: SignalData::Annotations(vec![annotation_record]),
        };

        let mut digital_header = signal_header(-1.0, 1.0, -1, 1);
        digital_header.samples_per_record = 2;
        let digital_signal = Signal {
            header: digital_header.clone(),
            data: SignalData::Digital(vec![11, 12]),
        };

        assert!(annotation_signal.digital_samples().is_none());
        assert!(digital_signal.annotation_records().is_none());

        let file = EdfFile {
            header: EdfHeader {
                version: "0".to_owned(),
                patient_id: String::new(),
                recording_id: String::new(),
                start: EdfDateTime {
                    year: 2024,
                    year_two_digits: 24,
                    month: 1,
                    day: 1,
                    hour: 0,
                    minute: 0,
                    second: 0,
                },
                header_bytes: 768,
                reserved: String::new(),
                declared_record_count: Some(1),
                record_duration_seconds: 1.0,
                signals: vec![annotation_header, digital_header],
            },
            signals: vec![annotation_signal, digital_signal],
            record_count: 1,
            trailing_data_bytes: 0,
        };
        let record = file.record(0).expect("record");

        assert!(record.digital_samples(0).is_none());
        assert!(record.annotation_record(1).is_none());
        assert!(record.annotations(1).is_none());
        assert_eq!(record.digital_samples(1), Some(&[11, 12][..]));
        assert_eq!(
            record
                .annotation_record(0)
                .expect("annotation record")
                .annotations[0]
                .text,
            "event"
        );
    }

    #[test]
    fn physical_iterator_conservatively_validates_the_full_i16_domain() {
        let header = signal_header(0.0, 9e307, 0, 1);
        assert_eq!(header.gain(), Ok(9e307));
        assert_eq!(header.offset(), Ok(0.0));
        assert_eq!(header.physical_value(0), Ok(0.0));
        assert_eq!(
            header.physical_value(2),
            Err(CalibrationError::NonFiniteResult)
        );

        let signal = Signal {
            header,
            data: SignalData::Digital(vec![0]),
        };
        assert_eq!(
            signal
                .physical_samples()
                .expect_err("the wire domain can overflow even when stored samples do not"),
            CalibrationError::NonFiniteResult
        );
    }

    #[test]
    fn non_finite_gain_and_offset_report_the_exact_error_category() {
        let gain_overflow = signal_header(-f64::MAX, f64::MAX, -1, 1);
        assert_eq!(gain_overflow.gain(), Err(CalibrationError::NonFiniteResult));
        assert_eq!(
            gain_overflow.offset(),
            Err(CalibrationError::NonFiniteResult)
        );
        assert_eq!(
            gain_overflow.physical_value(0),
            Err(CalibrationError::NonFiniteResult)
        );

        let offset_overflow = signal_header(1e308, 0.0, 32_767, 32_766);
        assert_eq!(offset_overflow.gain(), Ok(1e308));
        assert_eq!(
            offset_overflow.offset(),
            Err(CalibrationError::NonFiniteResult)
        );
        assert_eq!(
            offset_overflow.physical_value(0),
            Err(CalibrationError::NonFiniteResult)
        );
    }
}
