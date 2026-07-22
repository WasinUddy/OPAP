// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (c) 2026 OPAP contributors
// OSCAR/SleepyHead attribution is documented in this crate's README.

use core::fmt;

/// A parser failure with the byte and optional signal/record location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset in the EDF input at which the failure was detected.
    pub offset: usize,
    /// Signal descriptor or data index, when applicable.
    pub signal_index: Option<usize>,
    /// Data-record index, when applicable.
    pub record_index: Option<usize>,
    /// Machine-readable error category.
    pub kind: ParseErrorKind,
}

impl ParseError {
    pub(crate) const fn new(offset: usize, kind: ParseErrorKind) -> Self {
        Self {
            offset,
            signal_index: None,
            record_index: None,
            kind,
        }
    }

    pub(crate) const fn signal(mut self, signal_index: usize) -> Self {
        self.signal_index = Some(signal_index);
        self
    }

    pub(crate) const fn record(mut self, record_index: usize) -> Self {
        self.record_index = Some(record_index);
        self
    }
}

/// Specific reasons an EDF stream was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    UnexpectedEof {
        context: &'static str,
        needed: usize,
        available: usize,
    },
    InvalidAscii {
        field: &'static str,
    },
    InvalidNumber {
        field: &'static str,
        value: String,
    },
    ValueOutOfRange {
        field: &'static str,
        value: String,
    },
    InvalidDateTime {
        value: String,
    },
    HeaderLengthMismatch {
        declared: usize,
        expected: usize,
    },
    DataLengthMismatch {
        expected: usize,
        available: usize,
    },
    UnknownRecordCountWithEmptyRecord,
    ZeroByteRecords {
        record_count: usize,
    },
    ArithmeticOverflow {
        operation: &'static str,
    },
    LimitExceeded {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    MissingTimekeepingSignal,
    MissingRecordTimekeepingOnset,
    MalformedAnnotation {
        reason: &'static str,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "EDF parse error at byte {}", self.offset)?;
        if let Some(signal) = self.signal_index {
            write!(formatter, ", signal {signal}")?;
        }
        if let Some(record) = self.record_index {
            write!(formatter, ", record {record}")?;
        }
        write!(formatter, ": ")?;

        match &self.kind {
            ParseErrorKind::UnexpectedEof {
                context,
                needed,
                available,
            } => write!(
                formatter,
                "unexpected end of {context}; need {needed} bytes, have {available}"
            ),
            ParseErrorKind::InvalidAscii { field } => {
                write!(formatter, "{field} contains non-ASCII bytes")
            }
            ParseErrorKind::InvalidNumber { field, value } => {
                write!(formatter, "invalid numeric {field}: {value:?}")
            }
            ParseErrorKind::ValueOutOfRange { field, value } => {
                write!(formatter, "{field} is out of range: {value:?}")
            }
            ParseErrorKind::InvalidDateTime { value } => {
                write!(formatter, "invalid EDF start date/time: {value:?}")
            }
            ParseErrorKind::HeaderLengthMismatch { declared, expected } => write!(
                formatter,
                "declared header length {declared} does not match calculated length {expected}"
            ),
            ParseErrorKind::DataLengthMismatch {
                expected,
                available,
            } => write!(
                formatter,
                "record data length mismatch; expected {expected} bytes, have {available}"
            ),
            ParseErrorKind::UnknownRecordCountWithEmptyRecord => write!(
                formatter,
                "cannot infer an unknown record count when every signal has zero samples"
            ),
            ParseErrorKind::ZeroByteRecords { record_count } => write!(
                formatter,
                "declared {record_count} data records, but every record is zero bytes"
            ),
            ParseErrorKind::ArithmeticOverflow { operation } => {
                write!(formatter, "integer overflow while calculating {operation}")
            }
            ParseErrorKind::LimitExceeded {
                resource,
                limit,
                actual,
            } => write!(
                formatter,
                "{resource} limit exceeded; limit is {limit}, requested {actual}"
            ),
            ParseErrorKind::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not reserve capacity for {requested} {resource}"
            ),
            ParseErrorKind::MissingTimekeepingSignal => formatter
                .write_str("EDF+D requires a primary signal labeled exactly 'EDF Annotations'"),
            ParseErrorKind::MissingRecordTimekeepingOnset => formatter
                .write_str("EDF+D record is missing its leading empty timekeeping annotation"),
            ParseErrorKind::MalformedAnnotation { reason } => {
                write!(formatter, "malformed EDF+ annotation: {reason}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// A signal header cannot map digital samples to physical values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationError {
    EqualDigitalBounds,
    NonFiniteResult,
    NotDigitalSignal,
}

impl fmt::Display for CalibrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EqualDigitalBounds => {
                formatter.write_str("digital minimum and maximum must differ")
            }
            Self::NonFiniteResult => formatter.write_str("physical calibration is not finite"),
            Self::NotDigitalSignal => {
                formatter.write_str("annotation signals have no physical samples")
            }
        }
    }
}

impl std::error::Error for CalibrationError {}
