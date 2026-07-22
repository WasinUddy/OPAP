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
