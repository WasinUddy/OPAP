// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2025 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from OSCAR:
// https://gitlab.com/CrimsonNape/OSCAR-code
// Upstream commit: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
// Relevant upstream file:
// oscar/SleepLib/loader_plugins/resmed_loader.cpp
// Modified: 2026-07-23

//! Bounded decoding of ResMed STR therapy-day summaries.
//!
//! ResMed stores these values once per therapy day, not once per mask-on
//! interval. This decoder therefore deliberately returns a day-scoped model
//! and never constructs a [`crate::domain::SessionSummary`]. In particular, a
//! caller must not copy one day's reported indices or statistics into every
//! session on a multi-session day.
//! The decoder also has no device-clock input: it preserves every declared
//! record, including a possibly mutable current/future record, as a reported
//! day value rather than claiming it is final. Integration must align the
//! record with [`super::str::StrBoundaryIndex`] before filtering or presenting
//! completeness.
//!
//! Signal lookup follows the exact, case-sensitive, left-to-right label order
//! used by the pinned OSCAR loader. OSCAR selects occurrence zero when a label
//! is repeated; OPAP retains that selection rule but emits a typed diagnostic.
//! Unlike the pinned implementation, OPAP applies the complete EDF affine
//! calibration before the ResMed-specific leak and tidal-volume unit scales.

#![allow(
    dead_code,
    reason = "the day-scoped decoder is staged before importer/storage attribution is wired"
)]

use crate::domain::DeviceLocalDateTime;
use opap_edf::{CalibrationError, EdfFile, Limits, ParseError, ParseErrorKind, Parser};
use serde::Serialize;
use std::{error, fmt};

const SECONDS_PER_DAY: f64 = 86_400.0;
const MAX_SIGNALS: usize = 256;
const MAX_RECORDS: usize = 20_000;
const MAX_SIGNAL_RECORDS: usize = MAX_SIGNALS * MAX_RECORDS;
const MAX_TOTAL_SAMPLES: usize = 16_000_000;
const MAX_REPORTED_VALUES: usize = MAX_RECORDS * ALL_METRICS.len();
const MAX_WARNINGS: usize = ALL_METRICS.len() * 4;

/// Largest complete, uncompressed root `STR.edf` accepted by this decoder.
pub(super) const RESMED_STR_SUMMARY_MAX_FILE_BYTES: usize = 32 * 1024 * 1024;

const STR_SUMMARY_LIMITS: Limits = Limits {
    max_signals: MAX_SIGNALS,
    max_records: MAX_RECORDS,
    max_signal_records: MAX_SIGNAL_RECORDS,
    max_total_samples: MAX_TOTAL_SAMPLES,
    max_annotation_bytes: 0,
    max_annotation_records: 0,
    max_annotations: 0,
    max_annotation_text_bytes: 0,
};

/// Caller-supplied identity policy for one root STR summary decode.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct StrSummaryDecodeOptions<'a> {
    /// Identification serial expected in the EDF `SRN=` recording token.
    pub(super) expected_serial: &'a str,
}

/// Whether the recording identity was checked against the selected card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StrSummarySerialVerification {
    /// The sole non-empty `SRN=` token exactly matched the expected serial.
    Verified,
}

/// Stable identity of one device-reported STR therapy-day value.
///
/// The enum is intentionally distinct from calculated analytics. The value
/// remains in this day-scoped type because the current generic
/// `SummaryMetric` is session-scoped. Its stable key retains the
/// `pap.summary.resmed` source namespace for a future day-summary domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StrSummaryMetricKind {
    MaskDuration,
    Ahi,
    Ai,
    Hi,
    Uai,
    Cai,
    Oai,
    Csr,
    LeakMedian,
    LeakP95,
    LeakMaximum,
    RespiratoryRateMedian,
    RespiratoryRateP95,
    RespiratoryRateMaximum,
    MinuteVentilationMedian,
    MinuteVentilationP95,
    MinuteVentilationMaximum,
    TidalVolumeMedian,
    TidalVolumeP95,
    TidalVolumeMaximum,
    MaskPressureMedian,
    MaskPressureP95,
    MaskPressureMaximum,
    TargetEpapMedian,
    TargetEpapP95,
    TargetEpapMaximum,
    TargetIpapMedian,
    TargetIpapP95,
    TargetIpapMaximum,
    IeRatioMedian,
    IeRatioP95,
    IeRatioMaximum,
}

impl StrSummaryMetricKind {
    /// Stable source-qualified key reserved for a future day-summary domain.
    #[must_use]
    pub(super) const fn key(self) -> &'static str {
        self.spec().key
    }

    /// Non-localized display label for this reported day value.
    #[must_use]
    pub(super) const fn label(self) -> &'static str {
        self.spec().label
    }

    /// Canonical OPAP unit symbol, if the metric is not unitless.
    #[must_use]
    pub(super) const fn unit(self) -> Option<&'static str> {
        self.spec().unit
    }

    const fn spec(self) -> &'static MetricSpec {
        match self {
            Self::MaskDuration => &MASK_DURATION,
            Self::Ahi => &AHI,
            Self::Ai => &AI,
            Self::Hi => &HI,
            Self::Uai => &UAI,
            Self::Cai => &CAI,
            Self::Oai => &OAI,
            Self::Csr => &CSR,
            Self::LeakMedian => &LEAK_MEDIAN,
            Self::LeakP95 => &LEAK_P95,
            Self::LeakMaximum => &LEAK_MAXIMUM,
            Self::RespiratoryRateMedian => &RESPIRATORY_RATE_MEDIAN,
            Self::RespiratoryRateP95 => &RESPIRATORY_RATE_P95,
            Self::RespiratoryRateMaximum => &RESPIRATORY_RATE_MAXIMUM,
            Self::MinuteVentilationMedian => &MINUTE_VENTILATION_MEDIAN,
            Self::MinuteVentilationP95 => &MINUTE_VENTILATION_P95,
            Self::MinuteVentilationMaximum => &MINUTE_VENTILATION_MAXIMUM,
            Self::TidalVolumeMedian => &TIDAL_VOLUME_MEDIAN,
            Self::TidalVolumeP95 => &TIDAL_VOLUME_P95,
            Self::TidalVolumeMaximum => &TIDAL_VOLUME_MAXIMUM,
            Self::MaskPressureMedian => &MASK_PRESSURE_MEDIAN,
            Self::MaskPressureP95 => &MASK_PRESSURE_P95,
            Self::MaskPressureMaximum => &MASK_PRESSURE_MAXIMUM,
            Self::TargetEpapMedian => &TARGET_EPAP_MEDIAN,
            Self::TargetEpapP95 => &TARGET_EPAP_P95,
            Self::TargetEpapMaximum => &TARGET_EPAP_MAXIMUM,
            Self::TargetIpapMedian => &TARGET_IPAP_MEDIAN,
            Self::TargetIpapP95 => &TARGET_IPAP_P95,
            Self::TargetIpapMaximum => &TARGET_IPAP_MAXIMUM,
            Self::IeRatioMedian => &IE_RATIO_MEDIAN,
            Self::IeRatioP95 => &IE_RATIO_P95,
            Self::IeRatioMaximum => &IE_RATIO_MAXIMUM,
        }
    }
}

/// One compact value reported in a ResMed therapy-day record.
///
/// The source record may describe an in-progress current day; this decoder has
/// no clock input and does not upgrade it to a "complete day" claim.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(super) struct StrReportedSummaryValue {
    pub(super) kind: StrSummaryMetricKind,
    /// Calibrated and unit-normalized value.
    pub(super) value: f64,
}

/// Provenance for one selected exact EDF signal.
///
/// A selection is shared by every day record. `source_occurrence` is zero for
/// every metric in the pinned loader but remains explicit to prevent a later
/// integration from silently changing duplicate-label behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct StrSummarySignalSelection {
    pub(super) kind: StrSummaryMetricKind,
    pub(super) source_label: String,
    pub(super) source_occurrence: u16,
    pub(super) source_signal_index: u16,
}

/// Device-reported values for one local-noon STR therapy-day record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct StrTherapyDaySummary {
    /// Zero-based EDF data-record index.
    pub(super) record_index: usize,
    /// Device-local start of this therapy day. ResMed STR records start at
    /// noon; no timezone or UTC offset is implied.
    pub(super) local_noon: DeviceLocalDateTime,
    /// Device-reported day aggregates in stable [`StrSummaryMetricKind`] order.
    pub(super) reported_values: Vec<StrReportedSummaryValue>,
}

/// Why a selected or expected reported field was omitted or was ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum StrSummaryWarningKind {
    /// Neither exact label in the pinned priority list was present.
    MissingSignal,
    /// Pinned OSCAR occurrence zero was used and later exact duplicates were
    /// ignored.
    DuplicateSignal { ignored_occurrences: u16 },
    /// A daily summary signal did not contain exactly one sample per record.
    InvalidSamplesPerRecord { actual: u32 },
    /// The EDF digital-to-physical mapping could not be evaluated.
    InvalidCalibration {
        reason: StrSummaryCalibrationFailure,
    },
    /// One or more daily samples were not valid non-negative reported values.
    InvalidValue {
        reason: StrSummaryInvalidValueReason,
    },
}

/// Privacy-safe calibration failure category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StrSummaryCalibrationFailure {
    EqualDigitalBounds,
    NonFiniteResult,
    NonDigitalSignal,
}

impl From<CalibrationError> for StrSummaryCalibrationFailure {
    fn from(value: CalibrationError) -> Self {
        match value {
            CalibrationError::EqualDigitalBounds => Self::EqualDigitalBounds,
            CalibrationError::NonFiniteResult => Self::NonFiniteResult,
            CalibrationError::NotDigitalSignal => Self::NonDigitalSignal,
        }
    }
}

/// Why a calibrated per-day value was omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StrSummaryInvalidValueReason {
    /// The raw sample fell outside the signal's declared EDF digital domain.
    DigitalSampleOutOfRange,
    Negative,
    NonFiniteAfterScaling,
}

/// Bounded, typed diagnostic for one reported metric.
///
/// Diagnostics never include recording identifiers, expected serials, source
/// paths, raw EDF text, or raw sample values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(super) struct StrSummaryWarning {
    pub(super) metric: StrSummaryMetricKind,
    pub(super) kind: StrSummaryWarningKind,
    /// Number of records affected. Selection-level warnings affect every
    /// declared record.
    pub(super) affected_records: u32,
    /// First affected record, when at least one record exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) first_record_index: Option<u32>,
}

/// Complete bounded result for one uncompressed root `STR.edf`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct StrSummaryIndex {
    pub(super) serial_verification: StrSummarySerialVerification,
    /// Exact signal provenance, stored once rather than repeated on every day.
    pub(super) selected_signals: Vec<StrSummarySignalSelection>,
    /// One entry per EDF record, including days with no usable reported values.
    pub(super) days: Vec<StrTherapyDaySummary>,
    /// At most four aggregate diagnostics per known metric.
    pub(super) warnings: Vec<StrSummaryWarning>,
}

/// Failure to decode a trustworthy STR summary source.
///
/// These categories deliberately discard the EDF parser's field values. EDF
/// headers are device-controlled input and can contain identifiers or other
/// private text that must not escape through `Debug`, `Display`, or an error
/// source chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StrSummaryParseFailure {
    UnexpectedEof,
    InvalidAscii,
    InvalidNumber,
    ValueOutOfRange,
    InvalidDateTime,
    HeaderLengthMismatch,
    DataLengthMismatch,
    UnknownRecordCountWithEmptyRecord,
    ZeroByteRecords,
    ArithmeticOverflow,
    LimitExceeded,
    AllocationFailed,
    MissingTimekeepingSignal,
    MissingRecordTimekeepingOnset,
    InvalidFirstRecordTimekeepingOnset,
    NonContiguousRecordTimekeepingOnset,
    MalformedAnnotation,
    OtherMalformedInput,
}

impl fmt::Display for StrSummaryParseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnexpectedEof => "unexpected end of input",
            Self::InvalidAscii => "invalid ASCII header field",
            Self::InvalidNumber => "invalid numeric header field",
            Self::ValueOutOfRange => "header value is out of range",
            Self::InvalidDateTime => "invalid start date or time",
            Self::HeaderLengthMismatch => "header length mismatch",
            Self::DataLengthMismatch => "record data length mismatch",
            Self::UnknownRecordCountWithEmptyRecord => {
                "unknown record count with an empty record layout"
            }
            Self::ZeroByteRecords => "zero-byte record layout",
            Self::ArithmeticOverflow => "size calculation overflow",
            Self::LimitExceeded => "parser resource limit exceeded",
            Self::AllocationFailed => "parser allocation failed",
            Self::MissingTimekeepingSignal => "missing EDF+ timekeeping signal",
            Self::MissingRecordTimekeepingOnset => "missing EDF+ record timekeeping onset",
            Self::InvalidFirstRecordTimekeepingOnset => {
                "invalid first EDF+ record timekeeping onset"
            }
            Self::NonContiguousRecordTimekeepingOnset => {
                "non-contiguous EDF+ record timekeeping onset"
            }
            Self::MalformedAnnotation => "malformed EDF+ annotation",
            Self::OtherMalformedInput => "malformed EDF input",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StrSummaryDecodeError {
    FileTooLarge {
        limit: usize,
        actual: usize,
    },
    EmptyExpectedSerial,
    Parse(StrSummaryParseFailure),
    UnsupportedEdfPlus,
    TrailingData {
        bytes: usize,
    },
    UnknownRecordCount,
    EmptyRecordSet,
    InvalidRecordDuration,
    InvalidRecordStart,
    MissingSerial,
    AmbiguousSerial,
    SerialMismatch,
    OutputLimitExceeded {
        limit: usize,
        actual: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    DateRange,
}

impl fmt::Display for StrSummaryDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { limit, actual } => write!(
                formatter,
                "STR summary EDF exceeds the {limit}-byte input limit ({actual} bytes)"
            ),
            Self::EmptyExpectedSerial => {
                formatter.write_str("expected STR summary serial must not be empty")
            }
            Self::Parse(source) => write!(formatter, "could not parse STR summary EDF: {source}"),
            Self::UnsupportedEdfPlus => {
                formatter.write_str("STR summary input must be plain EDF, not EDF+")
            }
            Self::TrailingData { bytes } => {
                write!(formatter, "STR summary EDF has {bytes} trailing bytes")
            }
            Self::UnknownRecordCount => {
                formatter.write_str("STR summary EDF must declare its daily record count")
            }
            Self::EmptyRecordSet => {
                formatter.write_str("STR summary EDF must contain at least one daily record")
            }
            Self::InvalidRecordDuration => {
                formatter.write_str("STR summary EDF records must each span exactly one day")
            }
            Self::InvalidRecordStart => {
                formatter.write_str("STR summary EDF must start at device-local noon")
            }
            Self::MissingSerial => {
                formatter.write_str("STR summary EDF recording identifier has no serial")
            }
            Self::AmbiguousSerial => formatter
                .write_str("STR summary EDF recording identifier has multiple serial tokens"),
            Self::SerialMismatch => {
                formatter.write_str("STR summary EDF serial does not match the selected device")
            }
            Self::OutputLimitExceeded { limit, actual } => write!(
                formatter,
                "STR summary output value limit exceeded; limit is {limit}, requested {actual}"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not reserve capacity for {requested} {resource}"
            ),
            Self::DateRange => formatter.write_str("STR summary therapy-day date is out of range"),
        }
    }
}

impl error::Error for StrSummaryDecodeError {}

impl From<ParseError> for StrSummaryDecodeError {
    fn from(source: ParseError) -> Self {
        let failure = match source.kind {
            ParseErrorKind::UnexpectedEof { .. } => StrSummaryParseFailure::UnexpectedEof,
            ParseErrorKind::InvalidAscii { .. } => StrSummaryParseFailure::InvalidAscii,
            ParseErrorKind::InvalidNumber { .. } => StrSummaryParseFailure::InvalidNumber,
            ParseErrorKind::ValueOutOfRange { .. } => StrSummaryParseFailure::ValueOutOfRange,
            ParseErrorKind::InvalidDateTime { .. } => StrSummaryParseFailure::InvalidDateTime,
            ParseErrorKind::HeaderLengthMismatch { .. } => {
                StrSummaryParseFailure::HeaderLengthMismatch
            }
            ParseErrorKind::DataLengthMismatch { .. } => StrSummaryParseFailure::DataLengthMismatch,
            ParseErrorKind::UnknownRecordCountWithEmptyRecord => {
                StrSummaryParseFailure::UnknownRecordCountWithEmptyRecord
            }
            ParseErrorKind::ZeroByteRecords { .. } => StrSummaryParseFailure::ZeroByteRecords,
            ParseErrorKind::ArithmeticOverflow { .. } => StrSummaryParseFailure::ArithmeticOverflow,
            ParseErrorKind::LimitExceeded { .. } => StrSummaryParseFailure::LimitExceeded,
            ParseErrorKind::AllocationFailed { .. } => StrSummaryParseFailure::AllocationFailed,
            ParseErrorKind::MissingTimekeepingSignal => {
                StrSummaryParseFailure::MissingTimekeepingSignal
            }
            ParseErrorKind::MissingRecordTimekeepingOnset => {
                StrSummaryParseFailure::MissingRecordTimekeepingOnset
            }
            ParseErrorKind::InvalidFirstRecordTimekeepingOnset => {
                StrSummaryParseFailure::InvalidFirstRecordTimekeepingOnset
            }
            ParseErrorKind::NonContiguousRecordTimekeepingOnset => {
                StrSummaryParseFailure::NonContiguousRecordTimekeepingOnset
            }
            ParseErrorKind::MalformedAnnotation { .. } => {
                StrSummaryParseFailure::MalformedAnnotation
            }
            _ => StrSummaryParseFailure::OtherMalformedInput,
        };
        Self::Parse(failure)
    }
}

#[derive(Debug, Clone, Copy)]
struct MetricSpec {
    kind: StrSummaryMetricKind,
    key: &'static str,
    label: &'static str,
    unit: Option<&'static str>,
    source_labels: &'static [&'static str],
    scale: f64,
}

macro_rules! metric {
    (
        $name:ident,
        $kind:ident,
        $key:literal,
        $label:literal,
        $unit:expr,
        $scale:expr,
        [$($source:literal),+ $(,)?]
    ) => {
        const $name: MetricSpec = MetricSpec {
            kind: StrSummaryMetricKind::$kind,
            key: $key,
            label: $label,
            unit: $unit,
            source_labels: &[$($source),+],
            scale: $scale,
        };
    };
}

metric!(
    MASK_DURATION,
    MaskDuration,
    "pap.summary.resmed.mask_duration",
    "Device-reported mask duration",
    Some("h"),
    1.0,
    ["Mask Dur", "Duration"]
);
metric!(
    AHI,
    Ahi,
    "pap.summary.resmed.ahi",
    "Device-reported AHI",
    Some("events/h"),
    1.0,
    ["AHI"]
);
metric!(
    AI,
    Ai,
    "pap.summary.resmed.ai",
    "Device-reported apnea index",
    Some("events/h"),
    1.0,
    ["AI"]
);
metric!(
    HI,
    Hi,
    "pap.summary.resmed.hi",
    "Device-reported hypopnea index",
    Some("events/h"),
    1.0,
    ["HI"]
);
metric!(
    UAI,
    Uai,
    "pap.summary.resmed.uai",
    "Device-reported unclassified apnea index",
    Some("events/h"),
    1.0,
    ["UAI"]
);
metric!(
    CAI,
    Cai,
    "pap.summary.resmed.cai",
    "Device-reported central apnea index",
    Some("events/h"),
    1.0,
    ["CAI"]
);
metric!(
    OAI,
    Oai,
    "pap.summary.resmed.oai",
    "Device-reported obstructive apnea index",
    Some("events/h"),
    1.0,
    ["OAI"]
);
metric!(
    CSR,
    Csr,
    "pap.summary.resmed.csr",
    "Device-reported Cheyne-Stokes respiration",
    Some("%"),
    1.0,
    ["CSR"]
);
metric!(
    LEAK_MEDIAN,
    LeakMedian,
    "pap.summary.resmed.leak_rate.median",
    "Device-reported median leak rate",
    Some("L/min"),
    60.0,
    ["Leak Med", "Leak.50"]
);
metric!(
    LEAK_P95,
    LeakP95,
    "pap.summary.resmed.leak_rate.p95",
    "Device-reported 95th percentile leak rate",
    Some("L/min"),
    60.0,
    ["Leak 95", "Leak.95"]
);
metric!(
    LEAK_MAXIMUM,
    LeakMaximum,
    "pap.summary.resmed.leak_rate.maximum",
    "Device-reported maximum leak rate",
    Some("L/min"),
    60.0,
    ["Leak Max", "Leak.Max"]
);
metric!(
    RESPIRATORY_RATE_MEDIAN,
    RespiratoryRateMedian,
    "pap.summary.resmed.respiratory_rate.median",
    "Device-reported median respiratory rate",
    Some("breaths/min"),
    1.0,
    ["RespRate.50", "RR Med"]
);
metric!(
    RESPIRATORY_RATE_P95,
    RespiratoryRateP95,
    "pap.summary.resmed.respiratory_rate.p95",
    "Device-reported 95th percentile respiratory rate",
    Some("breaths/min"),
    1.0,
    ["RespRate.95", "RR 95"]
);
metric!(
    RESPIRATORY_RATE_MAXIMUM,
    RespiratoryRateMaximum,
    "pap.summary.resmed.respiratory_rate.maximum",
    "Device-reported maximum respiratory rate",
    Some("breaths/min"),
    1.0,
    ["RespRate.Max", "RR Max"]
);
metric!(
    MINUTE_VENTILATION_MEDIAN,
    MinuteVentilationMedian,
    "pap.summary.resmed.minute_ventilation.median",
    "Device-reported median minute ventilation",
    Some("L/min"),
    1.0,
    ["MinVent.50", "Min Vent Med"]
);
metric!(
    MINUTE_VENTILATION_P95,
    MinuteVentilationP95,
    "pap.summary.resmed.minute_ventilation.p95",
    "Device-reported 95th percentile minute ventilation",
    Some("L/min"),
    1.0,
    ["MinVent.95", "Min Vent 95"]
);
metric!(
    MINUTE_VENTILATION_MAXIMUM,
    MinuteVentilationMaximum,
    "pap.summary.resmed.minute_ventilation.maximum",
    "Device-reported maximum minute ventilation",
    Some("L/min"),
    1.0,
    ["MinVent.Max", "Min Vent Max"]
);
metric!(
    TIDAL_VOLUME_MEDIAN,
    TidalVolumeMedian,
    "pap.summary.resmed.tidal_volume.median",
    "Device-reported median tidal volume",
    Some("mL"),
    1_000.0,
    ["TidVol.50", "Tid Vol Med"]
);
metric!(
    TIDAL_VOLUME_P95,
    TidalVolumeP95,
    "pap.summary.resmed.tidal_volume.p95",
    "Device-reported 95th percentile tidal volume",
    Some("mL"),
    1_000.0,
    ["TidVol.95", "Tid Vol 95"]
);
metric!(
    TIDAL_VOLUME_MAXIMUM,
    TidalVolumeMaximum,
    "pap.summary.resmed.tidal_volume.maximum",
    "Device-reported maximum tidal volume",
    Some("mL"),
    1_000.0,
    ["TidVol.Max", "Tid Vol Max"]
);
metric!(
    MASK_PRESSURE_MEDIAN,
    MaskPressureMedian,
    "pap.summary.resmed.mask_pressure.median",
    "Device-reported median mask pressure",
    Some("cmH2O"),
    1.0,
    ["MaskPress.50", "Mask Pres Med"]
);
metric!(
    MASK_PRESSURE_P95,
    MaskPressureP95,
    "pap.summary.resmed.mask_pressure.p95",
    "Device-reported 95th percentile mask pressure",
    Some("cmH2O"),
    1.0,
    ["MaskPress.95", "Mask Pres 95"]
);
metric!(
    MASK_PRESSURE_MAXIMUM,
    MaskPressureMaximum,
    "pap.summary.resmed.mask_pressure.maximum",
    "Device-reported maximum mask pressure",
    Some("cmH2O"),
    1.0,
    ["MaskPress.Max", "Mask Pres Max"]
);
metric!(
    TARGET_EPAP_MEDIAN,
    TargetEpapMedian,
    "pap.summary.resmed.target_epap.median",
    "Device-reported median target EPAP",
    Some("cmH2O"),
    1.0,
    ["TgtEPAP.50", "Exp Pres Med"]
);
metric!(
    TARGET_EPAP_P95,
    TargetEpapP95,
    "pap.summary.resmed.target_epap.p95",
    "Device-reported 95th percentile target EPAP",
    Some("cmH2O"),
    1.0,
    ["TgtEPAP.95", "Exp Pres 95"]
);
metric!(
    TARGET_EPAP_MAXIMUM,
    TargetEpapMaximum,
    "pap.summary.resmed.target_epap.maximum",
    "Device-reported maximum target EPAP",
    Some("cmH2O"),
    1.0,
    ["TgtEPAP.Max", "Exp Pres Max"]
);
metric!(
    TARGET_IPAP_MEDIAN,
    TargetIpapMedian,
    "pap.summary.resmed.target_ipap.median",
    "Device-reported median target IPAP",
    Some("cmH2O"),
    1.0,
    ["TgtIPAP.50", "Insp Pres Med"]
);
metric!(
    TARGET_IPAP_P95,
    TargetIpapP95,
    "pap.summary.resmed.target_ipap.p95",
    "Device-reported 95th percentile target IPAP",
    Some("cmH2O"),
    1.0,
    ["TgtIPAP.95", "Insp Pres 95"]
);
metric!(
    TARGET_IPAP_MAXIMUM,
    TargetIpapMaximum,
    "pap.summary.resmed.target_ipap.maximum",
    "Device-reported maximum target IPAP",
    Some("cmH2O"),
    1.0,
    ["TgtIPAP.Max", "Insp Pres Max"]
);
metric!(
    IE_RATIO_MEDIAN,
    IeRatioMedian,
    "pap.summary.resmed.ie_ratio.median",
    "Device-reported median I:E ratio",
    Some("ratio"),
    1.0,
    ["I:E Med"]
);
metric!(
    IE_RATIO_P95,
    IeRatioP95,
    "pap.summary.resmed.ie_ratio.p95",
    "Device-reported 95th percentile I:E ratio",
    Some("ratio"),
    1.0,
    ["I:E 95"]
);
metric!(
    IE_RATIO_MAXIMUM,
    IeRatioMaximum,
    "pap.summary.resmed.ie_ratio.maximum",
    "Device-reported maximum I:E ratio",
    Some("ratio"),
    1.0,
    ["I:E Max"]
);

const ALL_METRICS: [StrSummaryMetricKind; 32] = [
    StrSummaryMetricKind::MaskDuration,
    StrSummaryMetricKind::Ahi,
    StrSummaryMetricKind::Ai,
    StrSummaryMetricKind::Hi,
    StrSummaryMetricKind::Uai,
    StrSummaryMetricKind::Cai,
    StrSummaryMetricKind::Oai,
    StrSummaryMetricKind::Csr,
    StrSummaryMetricKind::LeakMedian,
    StrSummaryMetricKind::LeakP95,
    StrSummaryMetricKind::LeakMaximum,
    StrSummaryMetricKind::RespiratoryRateMedian,
    StrSummaryMetricKind::RespiratoryRateP95,
    StrSummaryMetricKind::RespiratoryRateMaximum,
    StrSummaryMetricKind::MinuteVentilationMedian,
    StrSummaryMetricKind::MinuteVentilationP95,
    StrSummaryMetricKind::MinuteVentilationMaximum,
    StrSummaryMetricKind::TidalVolumeMedian,
    StrSummaryMetricKind::TidalVolumeP95,
    StrSummaryMetricKind::TidalVolumeMaximum,
    StrSummaryMetricKind::MaskPressureMedian,
    StrSummaryMetricKind::MaskPressureP95,
    StrSummaryMetricKind::MaskPressureMaximum,
    StrSummaryMetricKind::TargetEpapMedian,
    StrSummaryMetricKind::TargetEpapP95,
    StrSummaryMetricKind::TargetEpapMaximum,
    StrSummaryMetricKind::TargetIpapMedian,
    StrSummaryMetricKind::TargetIpapP95,
    StrSummaryMetricKind::TargetIpapMaximum,
    StrSummaryMetricKind::IeRatioMedian,
    StrSummaryMetricKind::IeRatioP95,
    StrSummaryMetricKind::IeRatioMaximum,
];

#[derive(Debug)]
struct SelectedMetric {
    spec: &'static MetricSpec,
    signal_index: usize,
    gain: f64,
    offset: f64,
    digital_minimum: i32,
    digital_maximum: i32,
    invalid_digital_range_records: u32,
    first_invalid_digital_range_record: Option<u32>,
    invalid_negative_records: u32,
    first_negative_record: Option<u32>,
    invalid_non_finite_records: u32,
    first_non_finite_record: Option<u32>,
    usable: bool,
}

/// Decode device-reported, therapy-day-scoped summary values from a complete,
/// uncompressed root `STR.edf`.
///
/// Structural or identity failures reject this summary source. Missing and
/// malformed optional metrics are isolated to that metric and represented by
/// bounded typed warnings.
///
/// # Errors
///
/// Returns [`StrSummaryDecodeError`] when the input is over limit, malformed,
/// not a daily local-noon STR source, has trailing data, or fails requested
/// serial verification.
pub(super) fn decode_str_summaries(
    bytes: &[u8],
    options: StrSummaryDecodeOptions<'_>,
) -> Result<StrSummaryIndex, StrSummaryDecodeError> {
    if bytes.len() > RESMED_STR_SUMMARY_MAX_FILE_BYTES {
        return Err(StrSummaryDecodeError::FileTooLarge {
            limit: RESMED_STR_SUMMARY_MAX_FILE_BYTES,
            actual: bytes.len(),
        });
    }
    if options.expected_serial.is_empty() {
        return Err(StrSummaryDecodeError::EmptyExpectedSerial);
    }

    let parser = Parser::new(STR_SUMMARY_LIMITS);
    let header = parser.parse_header(bytes)?;
    validate_header_shape(&header)?;
    let serial_verification = verify_serial(&header.recording_id, options.expected_serial)?;
    let record_count = header
        .declared_record_count
        .ok_or(StrSummaryDecodeError::UnknownRecordCount)?;
    if record_count == 0 {
        return Err(StrSummaryDecodeError::EmptyRecordSet);
    }
    let output_bound = record_count.checked_mul(ALL_METRICS.len()).ok_or(
        StrSummaryDecodeError::OutputLimitExceeded {
            limit: MAX_REPORTED_VALUES,
            actual: usize::MAX,
        },
    )?;
    if output_bound > MAX_REPORTED_VALUES {
        return Err(StrSummaryDecodeError::OutputLimitExceeded {
            limit: MAX_REPORTED_VALUES,
            actual: output_bound,
        });
    }

    let parsed = parser.parse(bytes)?;
    if parsed.trailing_data_bytes() != 0 {
        return Err(StrSummaryDecodeError::TrailingData {
            bytes: parsed.trailing_data_bytes(),
        });
    }

    decode_summary_records(&parsed, serial_verification)
}

fn validate_header_shape(header: &opap_edf::EdfHeader) -> Result<(), StrSummaryDecodeError> {
    if header.is_continuous() || header.is_discontinuous() {
        return Err(StrSummaryDecodeError::UnsupportedEdfPlus);
    }
    if header.record_duration_seconds.to_bits() != SECONDS_PER_DAY.to_bits() {
        return Err(StrSummaryDecodeError::InvalidRecordDuration);
    }
    if header.start.hour != 12 || header.start.minute != 0 || header.start.second != 0 {
        return Err(StrSummaryDecodeError::InvalidRecordStart);
    }
    Ok(())
}

fn verify_serial(
    recording_id: &str,
    expected_serial: &str,
) -> Result<StrSummarySerialVerification, StrSummaryDecodeError> {
    let mut serials = recording_id
        .split_ascii_whitespace()
        .filter_map(|token| token.strip_prefix("SRN="))
        .filter(|serial| !serial.is_empty());
    let Some(actual) = serials.next() else {
        return Err(StrSummaryDecodeError::MissingSerial);
    };
    if serials.next().is_some() {
        return Err(StrSummaryDecodeError::AmbiguousSerial);
    }
    if actual != expected_serial {
        return Err(StrSummaryDecodeError::SerialMismatch);
    }
    Ok(StrSummarySerialVerification::Verified)
}

fn decode_summary_records(
    parsed: &EdfFile,
    serial_verification: StrSummarySerialVerification,
) -> Result<StrSummaryIndex, StrSummaryDecodeError> {
    let record_count_u32 =
        u32::try_from(parsed.record_count()).map_err(|_| StrSummaryDecodeError::DateRange)?;
    let first_record = (record_count_u32 > 0).then_some(0);

    let mut selections = Vec::new();
    selections
        .try_reserve_exact(ALL_METRICS.len())
        .map_err(|_| StrSummaryDecodeError::AllocationFailed {
            resource: "STR summary signal selections",
            requested: ALL_METRICS.len(),
        })?;
    let mut warnings = Vec::new();
    warnings.try_reserve_exact(MAX_WARNINGS).map_err(|_| {
        StrSummaryDecodeError::AllocationFailed {
            resource: "STR summary warnings",
            requested: MAX_WARNINGS,
        }
    })?;
    let mut selected = Vec::new();
    selected.try_reserve_exact(ALL_METRICS.len()).map_err(|_| {
        StrSummaryDecodeError::AllocationFailed {
            resource: "STR summary metric plans",
            requested: ALL_METRICS.len(),
        }
    })?;

    for kind in ALL_METRICS {
        let spec = kind.spec();
        let Some((signal_index, source_label, duplicate_count)) =
            select_signal(parsed, spec.source_labels)
        else {
            push_warning(
                &mut warnings,
                StrSummaryWarning {
                    metric: kind,
                    kind: StrSummaryWarningKind::MissingSignal,
                    affected_records: record_count_u32,
                    first_record_index: first_record,
                },
            );
            continue;
        };

        selections.push(StrSummarySignalSelection {
            kind,
            source_label: source_label.to_owned(),
            source_occurrence: 0,
            source_signal_index: u16::try_from(signal_index)
                .expect("parser signal bound fits source index"),
        });
        if duplicate_count > 0 {
            push_warning(
                &mut warnings,
                StrSummaryWarning {
                    metric: kind,
                    kind: StrSummaryWarningKind::DuplicateSignal {
                        ignored_occurrences: u16::try_from(duplicate_count).unwrap_or(u16::MAX),
                    },
                    affected_records: record_count_u32,
                    first_record_index: first_record,
                },
            );
        }

        let signal = parsed
            .signal(signal_index)
            .expect("selected header index resolves after full parse");
        let samples_per_record = signal.header.samples_per_record;
        if samples_per_record != 1 {
            push_warning(
                &mut warnings,
                StrSummaryWarning {
                    metric: kind,
                    kind: StrSummaryWarningKind::InvalidSamplesPerRecord {
                        actual: u32::try_from(samples_per_record).unwrap_or(u32::MAX),
                    },
                    affected_records: record_count_u32,
                    first_record_index: first_record,
                },
            );
            selected.push(SelectedMetric {
                spec,
                signal_index,
                gain: 0.0,
                offset: 0.0,
                digital_minimum: 0,
                digital_maximum: 0,
                invalid_digital_range_records: 0,
                first_invalid_digital_range_record: None,
                invalid_negative_records: 0,
                first_negative_record: None,
                invalid_non_finite_records: 0,
                first_non_finite_record: None,
                usable: false,
            });
            continue;
        }
        if signal.digital_samples().is_none() {
            push_warning(
                &mut warnings,
                StrSummaryWarning {
                    metric: kind,
                    kind: StrSummaryWarningKind::InvalidCalibration {
                        reason: StrSummaryCalibrationFailure::NonDigitalSignal,
                    },
                    affected_records: record_count_u32,
                    first_record_index: first_record,
                },
            );
            selected.push(SelectedMetric {
                spec,
                signal_index,
                gain: 0.0,
                offset: 0.0,
                digital_minimum: 0,
                digital_maximum: 0,
                invalid_digital_range_records: 0,
                first_invalid_digital_range_record: None,
                invalid_negative_records: 0,
                first_negative_record: None,
                invalid_non_finite_records: 0,
                first_non_finite_record: None,
                usable: false,
            });
            continue;
        }
        let gain = match signal.header.gain() {
            Ok(value) => value,
            Err(reason) => {
                push_warning(
                    &mut warnings,
                    StrSummaryWarning {
                        metric: kind,
                        kind: StrSummaryWarningKind::InvalidCalibration {
                            reason: reason.into(),
                        },
                        affected_records: record_count_u32,
                        first_record_index: first_record,
                    },
                );
                selected.push(SelectedMetric {
                    spec,
                    signal_index,
                    gain: 0.0,
                    offset: 0.0,
                    digital_minimum: 0,
                    digital_maximum: 0,
                    invalid_digital_range_records: 0,
                    first_invalid_digital_range_record: None,
                    invalid_negative_records: 0,
                    first_negative_record: None,
                    invalid_non_finite_records: 0,
                    first_non_finite_record: None,
                    usable: false,
                });
                continue;
            }
        };
        let offset = match signal.header.offset() {
            Ok(value) => value,
            Err(reason) => {
                push_warning(
                    &mut warnings,
                    StrSummaryWarning {
                        metric: kind,
                        kind: StrSummaryWarningKind::InvalidCalibration {
                            reason: reason.into(),
                        },
                        affected_records: record_count_u32,
                        first_record_index: first_record,
                    },
                );
                selected.push(SelectedMetric {
                    spec,
                    signal_index,
                    gain: 0.0,
                    offset: 0.0,
                    digital_minimum: 0,
                    digital_maximum: 0,
                    invalid_digital_range_records: 0,
                    first_invalid_digital_range_record: None,
                    invalid_negative_records: 0,
                    first_negative_record: None,
                    invalid_non_finite_records: 0,
                    first_non_finite_record: None,
                    usable: false,
                });
                continue;
            }
        };
        selected.push(SelectedMetric {
            spec,
            signal_index,
            gain,
            offset,
            digital_minimum: signal
                .header
                .digital_minimum
                .min(signal.header.digital_maximum),
            digital_maximum: signal
                .header
                .digital_minimum
                .max(signal.header.digital_maximum),
            invalid_digital_range_records: 0,
            first_invalid_digital_range_record: None,
            invalid_negative_records: 0,
            first_negative_record: None,
            invalid_non_finite_records: 0,
            first_non_finite_record: None,
            usable: true,
        });
    }

    let start = resmed_header_start(parsed.header().start)?;
    let start_day_number = days_from_civil(start.year, start.month, start.day);
    let usable_count = selected.iter().filter(|metric| metric.usable).count();
    let mut days = Vec::new();
    days.try_reserve_exact(parsed.record_count()).map_err(|_| {
        StrSummaryDecodeError::AllocationFailed {
            resource: "STR therapy-day summaries",
            requested: parsed.record_count(),
        }
    })?;

    for record in parsed.records() {
        let record_index_u32 =
            u32::try_from(record.index()).map_err(|_| StrSummaryDecodeError::DateRange)?;
        let record_index_i64 =
            i64::try_from(record.index()).map_err(|_| StrSummaryDecodeError::DateRange)?;
        let day_number = start_day_number
            .checked_add(record_index_i64)
            .ok_or(StrSummaryDecodeError::DateRange)?;
        let (year, month, day) =
            civil_from_days(day_number).ok_or(StrSummaryDecodeError::DateRange)?;
        let mut reported_values = Vec::new();
        reported_values
            .try_reserve_exact(usable_count)
            .map_err(|_| StrSummaryDecodeError::AllocationFailed {
                resource: "STR reported summary values",
                requested: usable_count,
            })?;

        for metric in &mut selected {
            if !metric.usable {
                continue;
            }
            let raw = record
                .digital_samples(metric.signal_index)
                .and_then(|samples| samples.first())
                .copied()
                .expect("validated scalar digital signal has one sample per record");
            if !(metric.digital_minimum..=metric.digital_maximum).contains(&i32::from(raw)) {
                metric.invalid_digital_range_records =
                    metric.invalid_digital_range_records.saturating_add(1);
                metric
                    .first_invalid_digital_range_record
                    .get_or_insert(record_index_u32);
                continue;
            }
            let physical = f64::from(raw) * metric.gain + metric.offset;
            let normalized = physical * metric.spec.scale;
            if !normalized.is_finite() {
                metric.invalid_non_finite_records =
                    metric.invalid_non_finite_records.saturating_add(1);
                metric
                    .first_non_finite_record
                    .get_or_insert(record_index_u32);
                continue;
            }
            if normalized < 0.0 {
                metric.invalid_negative_records = metric.invalid_negative_records.saturating_add(1);
                metric.first_negative_record.get_or_insert(record_index_u32);
                continue;
            }
            reported_values.push(StrReportedSummaryValue {
                kind: metric.spec.kind,
                value: normalized,
            });
        }

        days.push(StrTherapyDaySummary {
            record_index: record.index(),
            local_noon: DeviceLocalDateTime {
                year,
                month,
                day,
                hour: 12,
                minute: 0,
                second: 0,
                millisecond: 0,
            },
            reported_values,
        });
    }

    for metric in &selected {
        if metric.invalid_digital_range_records > 0 {
            push_warning(
                &mut warnings,
                StrSummaryWarning {
                    metric: metric.spec.kind,
                    kind: StrSummaryWarningKind::InvalidValue {
                        reason: StrSummaryInvalidValueReason::DigitalSampleOutOfRange,
                    },
                    affected_records: metric.invalid_digital_range_records,
                    first_record_index: metric.first_invalid_digital_range_record,
                },
            );
        }
        if metric.invalid_negative_records > 0 {
            push_warning(
                &mut warnings,
                StrSummaryWarning {
                    metric: metric.spec.kind,
                    kind: StrSummaryWarningKind::InvalidValue {
                        reason: StrSummaryInvalidValueReason::Negative,
                    },
                    affected_records: metric.invalid_negative_records,
                    first_record_index: metric.first_negative_record,
                },
            );
        }
        if metric.invalid_non_finite_records > 0 {
            push_warning(
                &mut warnings,
                StrSummaryWarning {
                    metric: metric.spec.kind,
                    kind: StrSummaryWarningKind::InvalidValue {
                        reason: StrSummaryInvalidValueReason::NonFiniteAfterScaling,
                    },
                    affected_records: metric.invalid_non_finite_records,
                    first_record_index: metric.first_non_finite_record,
                },
            );
        }
    }

    Ok(StrSummaryIndex {
        serial_verification,
        selected_signals: selections,
        days,
        warnings,
    })
}

fn select_signal(
    parsed: &EdfFile,
    labels: &'static [&'static str],
) -> Option<(usize, &'static str, usize)> {
    for &label in labels {
        let mut matches = parsed
            .signals()
            .iter()
            .enumerate()
            .filter(|(_, signal)| signal.header.label == label)
            .map(|(index, _)| index);
        if let Some(index) = matches.next() {
            return Some((index, label, matches.count()));
        }
    }
    None
}

fn push_warning(warnings: &mut Vec<StrSummaryWarning>, warning: StrSummaryWarning) {
    assert!(
        warnings.len() < MAX_WARNINGS,
        "STR summary warning-count invariant exceeded"
    );
    warnings.push(warning);
}

fn resmed_header_start(
    start: opap_edf::EdfDateTime,
) -> Result<DeviceLocalDateTime, StrSummaryDecodeError> {
    // Pinned OSCAR moves EDF's 1985-1999 interpretation into 2085-2099 for
    // ResMed STR files. Preserve that device-specific rule.
    let year = if start.year < 2000 {
        start
            .year
            .checked_add(100)
            .ok_or(StrSummaryDecodeError::DateRange)?
    } else {
        start.year
    };
    Ok(DeviceLocalDateTime {
        year,
        month: start.month,
        day: start.day,
        hour: start.hour,
        minute: start.minute,
        second: start.second,
        millisecond: 0,
    })
}

// Howard Hinnant's civil-calendar conversion, adjusted to the Unix epoch.
fn days_from_civil(year: u16, month: u8, day: u8) -> i64 {
    let mut year = i64::from(year);
    let month = i64::from(month);
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> Option<(u16, u8, u8)> {
    let zero_day = days.checked_add(719_468)?;
    let era = zero_day.div_euclid(146_097);
    let day_of_era = zero_day - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    Some((
        u16::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const SERIAL: &str = "serial-123";
    const EXPECTED_SOURCE_LABELS: &[(StrSummaryMetricKind, &[&str])] = &[
        (
            StrSummaryMetricKind::MaskDuration,
            &["Mask Dur", "Duration"],
        ),
        (StrSummaryMetricKind::Ahi, &["AHI"]),
        (StrSummaryMetricKind::Ai, &["AI"]),
        (StrSummaryMetricKind::Hi, &["HI"]),
        (StrSummaryMetricKind::Uai, &["UAI"]),
        (StrSummaryMetricKind::Cai, &["CAI"]),
        (StrSummaryMetricKind::Oai, &["OAI"]),
        (StrSummaryMetricKind::Csr, &["CSR"]),
        (StrSummaryMetricKind::LeakMedian, &["Leak Med", "Leak.50"]),
        (StrSummaryMetricKind::LeakP95, &["Leak 95", "Leak.95"]),
        (StrSummaryMetricKind::LeakMaximum, &["Leak Max", "Leak.Max"]),
        (
            StrSummaryMetricKind::RespiratoryRateMedian,
            &["RespRate.50", "RR Med"],
        ),
        (
            StrSummaryMetricKind::RespiratoryRateP95,
            &["RespRate.95", "RR 95"],
        ),
        (
            StrSummaryMetricKind::RespiratoryRateMaximum,
            &["RespRate.Max", "RR Max"],
        ),
        (
            StrSummaryMetricKind::MinuteVentilationMedian,
            &["MinVent.50", "Min Vent Med"],
        ),
        (
            StrSummaryMetricKind::MinuteVentilationP95,
            &["MinVent.95", "Min Vent 95"],
        ),
        (
            StrSummaryMetricKind::MinuteVentilationMaximum,
            &["MinVent.Max", "Min Vent Max"],
        ),
        (
            StrSummaryMetricKind::TidalVolumeMedian,
            &["TidVol.50", "Tid Vol Med"],
        ),
        (
            StrSummaryMetricKind::TidalVolumeP95,
            &["TidVol.95", "Tid Vol 95"],
        ),
        (
            StrSummaryMetricKind::TidalVolumeMaximum,
            &["TidVol.Max", "Tid Vol Max"],
        ),
        (
            StrSummaryMetricKind::MaskPressureMedian,
            &["MaskPress.50", "Mask Pres Med"],
        ),
        (
            StrSummaryMetricKind::MaskPressureP95,
            &["MaskPress.95", "Mask Pres 95"],
        ),
        (
            StrSummaryMetricKind::MaskPressureMaximum,
            &["MaskPress.Max", "Mask Pres Max"],
        ),
        (
            StrSummaryMetricKind::TargetEpapMedian,
            &["TgtEPAP.50", "Exp Pres Med"],
        ),
        (
            StrSummaryMetricKind::TargetEpapP95,
            &["TgtEPAP.95", "Exp Pres 95"],
        ),
        (
            StrSummaryMetricKind::TargetEpapMaximum,
            &["TgtEPAP.Max", "Exp Pres Max"],
        ),
        (
            StrSummaryMetricKind::TargetIpapMedian,
            &["TgtIPAP.50", "Insp Pres Med"],
        ),
        (
            StrSummaryMetricKind::TargetIpapP95,
            &["TgtIPAP.95", "Insp Pres 95"],
        ),
        (
            StrSummaryMetricKind::TargetIpapMaximum,
            &["TgtIPAP.Max", "Insp Pres Max"],
        ),
        (StrSummaryMetricKind::IeRatioMedian, &["I:E Med"]),
        (StrSummaryMetricKind::IeRatioP95, &["I:E 95"]),
        (StrSummaryMetricKind::IeRatioMaximum, &["I:E Max"]),
    ];

    #[derive(Debug, Clone)]
    struct SignalFixture {
        label: &'static str,
        dimension: &'static str,
        physical_minimum: &'static str,
        physical_maximum: &'static str,
        digital_minimum: &'static str,
        digital_maximum: &'static str,
        samples_per_record: usize,
        records: Vec<Vec<i16>>,
    }

    impl SignalFixture {
        fn scalar(label: &'static str, samples: &[i16]) -> Self {
            Self {
                label,
                dimension: "raw",
                physical_minimum: "-32768",
                physical_maximum: "32767",
                digital_minimum: "-32768",
                digital_maximum: "32767",
                samples_per_record: 1,
                records: samples.iter().map(|&sample| vec![sample]).collect(),
            }
        }

        fn per_record(label: &'static str, records: Vec<Vec<i16>>) -> Self {
            let samples_per_record = records.first().map_or(1, Vec::len);
            Self {
                label,
                dimension: "raw",
                physical_minimum: "-32768",
                physical_maximum: "32767",
                digital_minimum: "-32768",
                digital_maximum: "32767",
                samples_per_record,
                records,
            }
        }

        fn calibration(
            mut self,
            physical_minimum: &'static str,
            physical_maximum: &'static str,
            digital_minimum: &'static str,
            digital_maximum: &'static str,
        ) -> Self {
            self.physical_minimum = physical_minimum;
            self.physical_maximum = physical_maximum;
            self.digital_minimum = digital_minimum;
            self.digital_maximum = digital_maximum;
            self
        }
    }

    #[derive(Debug, Clone)]
    struct EdfFixture {
        start: &'static str,
        duration: &'static str,
        recording_id: &'static str,
        reserved: &'static str,
        actual_record_count: usize,
        declared_record_count: Option<&'static str>,
        signals: Vec<SignalFixture>,
    }

    impl EdfFixture {
        fn new(actual_record_count: usize, signals: Vec<SignalFixture>) -> Self {
            Self {
                start: "01.01.2612.00.00",
                duration: "86400",
                recording_id: "ResMed SRN=serial-123",
                reserved: "",
                actual_record_count,
                declared_record_count: None,
                signals,
            }
        }

        fn build(&self) -> Vec<u8> {
            assert_eq!(self.start.len(), 16);
            for signal in &self.signals {
                assert_eq!(signal.records.len(), self.actual_record_count);
                assert!(
                    signal
                        .records
                        .iter()
                        .all(|record| record.len() == signal.samples_per_record)
                );
            }

            let header_bytes = 256 + self.signals.len() * 256;
            let declared_records = self
                .declared_record_count
                .map_or_else(|| self.actual_record_count.to_string(), str::to_owned);
            let mut bytes = Vec::new();
            bytes.extend(field("0", 8));
            bytes.extend(field("patient", 80));
            bytes.extend(field(self.recording_id, 80));
            bytes.extend_from_slice(self.start.as_bytes());
            bytes.extend(field(&header_bytes.to_string(), 8));
            bytes.extend(field(self.reserved, 44));
            bytes.extend(field(&declared_records, 8));
            bytes.extend(field(self.duration, 8));
            bytes.extend(field(&self.signals.len().to_string(), 4));

            for signal in &self.signals {
                bytes.extend(field(signal.label, 16));
            }
            for _ in &self.signals {
                bytes.extend(field("", 80));
            }
            for signal in &self.signals {
                bytes.extend(field(signal.dimension, 8));
            }
            for signal in &self.signals {
                bytes.extend(field(signal.physical_minimum, 8));
            }
            for signal in &self.signals {
                bytes.extend(field(signal.physical_maximum, 8));
            }
            for signal in &self.signals {
                bytes.extend(field(signal.digital_minimum, 8));
            }
            for signal in &self.signals {
                bytes.extend(field(signal.digital_maximum, 8));
            }
            for _ in &self.signals {
                bytes.extend(field("", 80));
            }
            for signal in &self.signals {
                bytes.extend(field(&signal.samples_per_record.to_string(), 8));
            }
            for _ in &self.signals {
                bytes.extend(field("", 32));
            }
            assert_eq!(bytes.len(), header_bytes);

            for record_index in 0..self.actual_record_count {
                for signal in &self.signals {
                    for sample in &signal.records[record_index] {
                        bytes.extend_from_slice(&sample.to_le_bytes());
                    }
                }
            }
            bytes
        }
    }

    fn field(value: &str, width: usize) -> Vec<u8> {
        assert!(value.len() <= width);
        let mut output = vec![b' '; width];
        output[..value.len()].copy_from_slice(value.as_bytes());
        output
    }

    fn overwrite_field(bytes: &mut [u8], offset: usize, width: usize, value: &str) {
        let replacement = field(value, width);
        bytes[offset..offset + width].copy_from_slice(&replacement);
    }

    fn options(expected_serial: &str) -> StrSummaryDecodeOptions<'_> {
        StrSummaryDecodeOptions { expected_serial }
    }

    fn decode(bytes: &[u8]) -> StrSummaryIndex {
        decode_str_summaries(bytes, options(SERIAL)).expect("valid STR summary")
    }

    fn reported_value(
        decoded: &StrSummaryIndex,
        record_index: usize,
        kind: StrSummaryMetricKind,
    ) -> Option<f64> {
        decoded.days[record_index]
            .reported_values
            .iter()
            .find(|value| value.kind == kind)
            .map(|value| value.value)
    }

    fn warning(
        decoded: &StrSummaryIndex,
        metric: StrSummaryMetricKind,
        predicate: impl Fn(StrSummaryWarningKind) -> bool,
    ) -> &StrSummaryWarning {
        decoded
            .warnings
            .iter()
            .find(|warning| warning.metric == metric && predicate(warning.kind))
            .expect("expected typed warning")
    }

    #[test]
    fn fixture_writes_record_major_signal_slices() {
        let bytes = EdfFixture::new(
            2,
            vec![
                SignalFixture::per_record("one", vec![vec![1, 2], vec![3, 4]]),
                SignalFixture::scalar("two", &[10, 20]),
            ],
        )
        .build();
        let parsed = Parser::new(STR_SUMMARY_LIMITS)
            .parse(&bytes)
            .expect("fixture layout");
        assert_eq!(
            parsed.record(0).unwrap().digital_samples(0),
            Some(&[1, 2][..])
        );
        assert_eq!(
            parsed.record(0).unwrap().digital_samples(1),
            Some(&[10][..])
        );
        assert_eq!(
            parsed.record(1).unwrap().digital_samples(0),
            Some(&[3, 4][..])
        );
        assert_eq!(
            parsed.record(1).unwrap().digital_samples(1),
            Some(&[20][..])
        );
    }

    #[test]
    fn decodes_every_reported_field_with_stable_keys_units_and_scales() {
        let signals = EXPECTED_SOURCE_LABELS
            .iter()
            .map(|(_, labels)| SignalFixture::scalar(labels[0], &[2]))
            .collect();
        let decoded = decode(&EdfFixture::new(1, signals).build());

        assert_eq!(
            decoded.serial_verification,
            StrSummarySerialVerification::Verified
        );
        assert_eq!(decoded.selected_signals.len(), 32);
        assert_eq!(decoded.days.len(), 1);
        assert_eq!(decoded.days[0].reported_values.len(), 32);
        assert!(decoded.warnings.is_empty());
        assert_eq!(
            decoded.days[0]
                .reported_values
                .iter()
                .map(|value| value.kind)
                .collect::<Vec<_>>(),
            ALL_METRICS
        );
        assert_eq!(EXPECTED_SOURCE_LABELS.len(), ALL_METRICS.len());
        for ((expected_kind, expected_labels), actual_kind) in
            EXPECTED_SOURCE_LABELS.iter().zip(ALL_METRICS)
        {
            assert_eq!(*expected_kind, actual_kind);
            assert_eq!(actual_kind.spec().source_labels, *expected_labels);
        }

        let expected_keys = [
            "pap.summary.resmed.mask_duration",
            "pap.summary.resmed.ahi",
            "pap.summary.resmed.ai",
            "pap.summary.resmed.hi",
            "pap.summary.resmed.uai",
            "pap.summary.resmed.cai",
            "pap.summary.resmed.oai",
            "pap.summary.resmed.csr",
            "pap.summary.resmed.leak_rate.median",
            "pap.summary.resmed.leak_rate.p95",
            "pap.summary.resmed.leak_rate.maximum",
            "pap.summary.resmed.respiratory_rate.median",
            "pap.summary.resmed.respiratory_rate.p95",
            "pap.summary.resmed.respiratory_rate.maximum",
            "pap.summary.resmed.minute_ventilation.median",
            "pap.summary.resmed.minute_ventilation.p95",
            "pap.summary.resmed.minute_ventilation.maximum",
            "pap.summary.resmed.tidal_volume.median",
            "pap.summary.resmed.tidal_volume.p95",
            "pap.summary.resmed.tidal_volume.maximum",
            "pap.summary.resmed.mask_pressure.median",
            "pap.summary.resmed.mask_pressure.p95",
            "pap.summary.resmed.mask_pressure.maximum",
            "pap.summary.resmed.target_epap.median",
            "pap.summary.resmed.target_epap.p95",
            "pap.summary.resmed.target_epap.maximum",
            "pap.summary.resmed.target_ipap.median",
            "pap.summary.resmed.target_ipap.p95",
            "pap.summary.resmed.target_ipap.maximum",
            "pap.summary.resmed.ie_ratio.median",
            "pap.summary.resmed.ie_ratio.p95",
            "pap.summary.resmed.ie_ratio.maximum",
        ];
        let expected_units = [
            Some("h"),
            Some("events/h"),
            Some("events/h"),
            Some("events/h"),
            Some("events/h"),
            Some("events/h"),
            Some("events/h"),
            Some("%"),
            Some("L/min"),
            Some("L/min"),
            Some("L/min"),
            Some("breaths/min"),
            Some("breaths/min"),
            Some("breaths/min"),
            Some("L/min"),
            Some("L/min"),
            Some("L/min"),
            Some("mL"),
            Some("mL"),
            Some("mL"),
            Some("cmH2O"),
            Some("cmH2O"),
            Some("cmH2O"),
            Some("cmH2O"),
            Some("cmH2O"),
            Some("cmH2O"),
            Some("cmH2O"),
            Some("cmH2O"),
            Some("cmH2O"),
            Some("ratio"),
            Some("ratio"),
            Some("ratio"),
        ];
        let expected_values: [f64; 32] = [
            2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 120.0, 120.0, 120.0, 2.0, 2.0, 2.0, 2.0, 2.0,
            2.0, 2_000.0, 2_000.0, 2_000.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0,
            2.0,
        ];

        let mut unique_keys = BTreeSet::new();
        for (index, value) in decoded.days[0].reported_values.iter().enumerate() {
            assert_eq!(value.kind.key(), expected_keys[index]);
            assert_eq!(value.kind.unit(), expected_units[index]);
            assert_eq!(value.value.to_bits(), expected_values[index].to_bits());
            assert!(value.kind.label().starts_with("Device-reported"));
            assert!(unique_keys.insert(value.kind.key()));
        }
    }

    #[test]
    fn exact_alias_priority_precedes_header_order_and_fallbacks_are_supported() {
        let mut preferred_and_fallback = Vec::new();
        for &(_, labels) in EXPECTED_SOURCE_LABELS {
            if labels.len() > 1 {
                // Put the fallback earlier in the EDF to prove label priority,
                // rather than global header order, chooses the source.
                preferred_and_fallback.push(SignalFixture::scalar(labels[1], &[9]));
            }
            preferred_and_fallback.push(SignalFixture::scalar(labels[0], &[1]));
        }
        let preferred = decode(&EdfFixture::new(1, preferred_and_fallback).build());
        for &(kind, labels) in EXPECTED_SOURCE_LABELS {
            let selection = preferred
                .selected_signals
                .iter()
                .find(|selection| selection.kind == kind)
                .unwrap();
            assert_eq!(selection.source_label, labels[0]);
            assert_eq!(selection.source_occurrence, 0);
            assert_eq!(
                reported_value(&preferred, 0, kind).unwrap().to_bits(),
                kind.spec().scale.to_bits()
            );
        }

        let fallback_only = EXPECTED_SOURCE_LABELS
            .iter()
            .map(|(_, labels)| SignalFixture::scalar(labels[labels.len() - 1], &[3]))
            .collect();
        let fallback = decode(&EdfFixture::new(1, fallback_only).build());
        for &(kind, labels) in EXPECTED_SOURCE_LABELS {
            let selection = fallback
                .selected_signals
                .iter()
                .find(|selection| selection.kind == kind)
                .unwrap();
            assert_eq!(selection.source_label, labels[labels.len() - 1]);
            assert_eq!(
                reported_value(&fallback, 0, kind).unwrap().to_bits(),
                (3.0 * kind.spec().scale).to_bits()
            );
        }
    }

    #[test]
    fn occurrence_zero_is_selected_and_duplicate_is_reported_without_raw_data() {
        let bytes = EdfFixture::new(
            1,
            vec![
                SignalFixture::scalar("AHI", &[4]),
                SignalFixture::scalar("unrelated", &[77]),
                SignalFixture::scalar("AHI", &[9]),
            ],
        )
        .build();
        let decoded = decode(&bytes);

        assert_eq!(
            reported_value(&decoded, 0, StrSummaryMetricKind::Ahi),
            Some(4.0)
        );
        let selection = decoded
            .selected_signals
            .iter()
            .find(|selection| selection.kind == StrSummaryMetricKind::Ahi)
            .unwrap();
        assert_eq!(selection.source_signal_index, 0);
        assert_eq!(selection.source_occurrence, 0);
        let duplicate = warning(&decoded, StrSummaryMetricKind::Ahi, |kind| {
            matches!(
                kind,
                StrSummaryWarningKind::DuplicateSignal {
                    ignored_occurrences: 1
                }
            )
        });
        assert_eq!(duplicate.affected_records, 1);
        let serialized = serde_json::to_string(duplicate).unwrap();
        assert!(!serialized.contains("77"));
        assert!(!serialized.contains(SERIAL));
    }

    #[test]
    fn calibration_is_full_affine_before_resmed_unit_scaling() {
        let affine = |label| SignalFixture::scalar(label, &[0]).calibration("10", "18", "-8", "8");
        let bytes = EdfFixture::new(
            1,
            vec![
                affine("Mask Dur"),
                affine("AHI"),
                SignalFixture::scalar("AI", &[5]).calibration("0", "10", "10", "0"),
                affine("CSR"),
                affine("Leak Med"),
                affine("RespRate.50"),
                affine("MinVent.50"),
                affine("TidVol.50"),
                affine("MaskPress.50"),
                affine("TgtEPAP.50"),
                affine("TgtIPAP.50"),
                affine("I:E Med"),
            ],
        )
        .build();
        let decoded = decode(&bytes);

        for kind in [
            StrSummaryMetricKind::MaskDuration,
            StrSummaryMetricKind::Ahi,
            StrSummaryMetricKind::Csr,
            StrSummaryMetricKind::RespiratoryRateMedian,
            StrSummaryMetricKind::MinuteVentilationMedian,
            StrSummaryMetricKind::MaskPressureMedian,
            StrSummaryMetricKind::TargetEpapMedian,
            StrSummaryMetricKind::TargetIpapMedian,
            StrSummaryMetricKind::IeRatioMedian,
        ] {
            assert_eq!(reported_value(&decoded, 0, kind), Some(14.0));
        }
        assert_eq!(
            reported_value(&decoded, 0, StrSummaryMetricKind::Ai),
            Some(5.0)
        );
        assert_eq!(
            reported_value(&decoded, 0, StrSummaryMetricKind::LeakMedian),
            Some(840.0)
        );
        assert_eq!(
            reported_value(&decoded, 0, StrSummaryMetricKind::TidalVolumeMedian),
            Some(14_000.0)
        );
    }

    #[test]
    fn missing_and_wrong_case_labels_are_omitted_with_bounded_typed_warnings() {
        let decoded = decode(&EdfFixture::new(1, vec![SignalFixture::scalar("ahi", &[5])]).build());
        assert!(decoded.days[0].reported_values.is_empty());
        assert!(decoded.selected_signals.is_empty());
        assert_eq!(decoded.warnings.len(), ALL_METRICS.len());
        let missing = warning(&decoded, StrSummaryMetricKind::Ahi, |kind| {
            kind == StrSummaryWarningKind::MissingSignal
        });
        assert_eq!(missing.affected_records, 1);
        assert_eq!(missing.first_record_index, Some(0));
        assert!(decoded.warnings.len() <= MAX_WARNINGS);
    }

    #[test]
    fn invalid_metrics_are_isolated_and_per_record_warnings_are_aggregated() {
        let bytes = EdfFixture::new(
            3,
            vec![
                SignalFixture::scalar("AHI", &[-1, 0, -2]),
                SignalFixture::scalar("AI", &[0, 0, 0]),
                SignalFixture::scalar("HI", &[1, 1, 1]).calibration("0", "1", "0", "0"),
                SignalFixture::per_record("UAI", vec![vec![1, 2], vec![3, 4], vec![5, 6]]),
                SignalFixture::per_record("CAI", vec![vec![], vec![], vec![]]),
                SignalFixture::scalar("OAI", &[20, 5, 20]).calibration("0", "10", "0", "10"),
                SignalFixture::scalar("Leak Med", &[1, 0, 1]).calibration("0", "1e307", "0", "1"),
            ],
        )
        .build();
        let decoded = decode(&bytes);

        assert_eq!(reported_value(&decoded, 0, StrSummaryMetricKind::Ahi), None);
        assert_eq!(
            reported_value(&decoded, 1, StrSummaryMetricKind::Ahi),
            Some(0.0)
        );
        assert_eq!(reported_value(&decoded, 2, StrSummaryMetricKind::Ahi), None);
        for day in 0..3 {
            assert_eq!(
                reported_value(&decoded, day, StrSummaryMetricKind::Ai),
                Some(0.0)
            );
            assert_eq!(
                reported_value(&decoded, day, StrSummaryMetricKind::Hi),
                None
            );
            assert_eq!(
                reported_value(&decoded, day, StrSummaryMetricKind::Uai),
                None
            );
            assert_eq!(
                reported_value(&decoded, day, StrSummaryMetricKind::Cai),
                None
            );
        }
        assert_eq!(reported_value(&decoded, 0, StrSummaryMetricKind::Oai), None);
        assert_eq!(
            reported_value(&decoded, 1, StrSummaryMetricKind::Oai),
            Some(5.0)
        );
        assert_eq!(reported_value(&decoded, 2, StrSummaryMetricKind::Oai), None);
        assert_eq!(
            reported_value(&decoded, 0, StrSummaryMetricKind::LeakMedian),
            None
        );
        assert_eq!(
            reported_value(&decoded, 1, StrSummaryMetricKind::LeakMedian),
            Some(0.0)
        );
        assert_eq!(
            reported_value(&decoded, 2, StrSummaryMetricKind::LeakMedian),
            None
        );

        let negative = warning(&decoded, StrSummaryMetricKind::Ahi, |kind| {
            kind == StrSummaryWarningKind::InvalidValue {
                reason: StrSummaryInvalidValueReason::Negative,
            }
        });
        assert_eq!(negative.affected_records, 2);
        assert_eq!(negative.first_record_index, Some(0));

        let calibration = warning(&decoded, StrSummaryMetricKind::Hi, |kind| {
            kind == StrSummaryWarningKind::InvalidCalibration {
                reason: StrSummaryCalibrationFailure::EqualDigitalBounds,
            }
        });
        assert_eq!(calibration.affected_records, 3);

        let shape = warning(&decoded, StrSummaryMetricKind::Uai, |kind| {
            kind == StrSummaryWarningKind::InvalidSamplesPerRecord { actual: 2 }
        });
        assert_eq!(shape.affected_records, 3);
        let empty_shape = warning(&decoded, StrSummaryMetricKind::Cai, |kind| {
            kind == StrSummaryWarningKind::InvalidSamplesPerRecord { actual: 0 }
        });
        assert_eq!(empty_shape.affected_records, 3);
        let digital_range = warning(&decoded, StrSummaryMetricKind::Oai, |kind| {
            kind == StrSummaryWarningKind::InvalidValue {
                reason: StrSummaryInvalidValueReason::DigitalSampleOutOfRange,
            }
        });
        assert_eq!(digital_range.affected_records, 2);
        assert_eq!(digital_range.first_record_index, Some(0));

        let non_finite = warning(&decoded, StrSummaryMetricKind::LeakMedian, |kind| {
            kind == StrSummaryWarningKind::InvalidValue {
                reason: StrSummaryInvalidValueReason::NonFiniteAfterScaling,
            }
        });
        assert_eq!(non_finite.affected_records, 2);
        assert_eq!(non_finite.first_record_index, Some(0));
        assert_eq!(decoded.warnings.len(), 31);
    }

    #[test]
    fn warning_bound_accounts_for_every_distinct_reason_without_suppression() {
        let selected =
            SignalFixture::scalar("Leak Med", &[0, 1, 3]).calibration("-1", "1e307", "0", "2");
        let duplicate = SignalFixture::scalar("Leak Med", &[0, 0, 0]);
        let decoded = decode(&EdfFixture::new(3, vec![selected, duplicate]).build());

        let leak_warnings = decoded
            .warnings
            .iter()
            .filter(|warning| warning.metric == StrSummaryMetricKind::LeakMedian)
            .collect::<Vec<_>>();
        assert_eq!(leak_warnings.len(), 4);
        assert!(leak_warnings.iter().any(|warning| matches!(
            warning.kind,
            StrSummaryWarningKind::DuplicateSignal {
                ignored_occurrences: 1
            }
        )));
        for reason in [
            StrSummaryInvalidValueReason::DigitalSampleOutOfRange,
            StrSummaryInvalidValueReason::Negative,
            StrSummaryInvalidValueReason::NonFiniteAfterScaling,
        ] {
            assert!(
                leak_warnings.iter().any(|warning| {
                    warning.kind == StrSummaryWarningKind::InvalidValue { reason }
                })
            );
        }
        assert_eq!(decoded.warnings.len(), 35);
        assert!(decoded.warnings.len() < MAX_WARNINGS);
    }

    #[test]
    fn multiple_records_remain_day_scoped_and_roll_across_leap_day() {
        let mut fixture = EdfFixture::new(
            3,
            vec![
                SignalFixture::scalar("AHI", &[1, 2, 3]),
                SignalFixture::scalar("Mask Dur", &[4, 5, 6]),
            ],
        );
        fixture.start = "28.02.2812.00.00";
        let decoded = decode(&fixture.build());

        assert_eq!(
            decoded
                .days
                .iter()
                .map(|day| (
                    day.record_index,
                    day.local_noon.year,
                    day.local_noon.month,
                    day.local_noon.day
                ))
                .collect::<Vec<_>>(),
            vec![(0, 2028, 2, 28), (1, 2028, 2, 29), (2, 2028, 3, 1)]
        );
        assert_eq!(
            reported_value(&decoded, 0, StrSummaryMetricKind::Ahi),
            Some(1.0)
        );
        assert_eq!(
            reported_value(&decoded, 1, StrSummaryMetricKind::Ahi),
            Some(2.0)
        );
        assert_eq!(
            reported_value(&decoded, 2, StrSummaryMetricKind::Ahi),
            Some(3.0)
        );
        assert_eq!(
            reported_value(&decoded, 0, StrSummaryMetricKind::MaskDuration),
            Some(4.0)
        );
        assert_eq!(
            reported_value(&decoded, 2, StrSummaryMetricKind::MaskDuration),
            Some(6.0)
        );
    }

    #[test]
    fn resmed_century_repair_matches_pinned_str_day_behavior() {
        let mut fixture = EdfFixture::new(3, vec![SignalFixture::scalar("AHI", &[1, 2, 3])]);
        fixture.start = "28.02.8812.00.00";
        let decoded = decode(&fixture.build());
        assert_eq!(
            decoded
                .days
                .iter()
                .map(|day| (
                    day.local_noon.year,
                    day.local_noon.month,
                    day.local_noon.day
                ))
                .collect::<Vec<_>>(),
            vec![(2088, 2, 28), (2088, 2, 29), (2088, 3, 1)]
        );
    }

    #[test]
    fn serial_verification_is_mandatory_exact_unambiguous_and_privacy_safe() {
        let mut fixture = EdfFixture::new(1, vec![SignalFixture::scalar("AHI", &[1])]);
        fixture.recording_id = "ResMed SRN=private-actual";
        let bytes = fixture.build();
        let verified = decode_str_summaries(&bytes, options("private-actual")).unwrap();
        let serialized = serde_json::to_string(&verified).unwrap();
        assert!(!serialized.contains("private-actual"));

        let mismatch = decode_str_summaries(&bytes, options("private-expected")).unwrap_err();
        assert_eq!(mismatch, StrSummaryDecodeError::SerialMismatch);
        let displayed = mismatch.to_string();
        assert!(!displayed.contains("private-actual"));
        assert!(!displayed.contains("private-expected"));

        fixture.recording_id = "ResMed SRN=private-actual SRN=private-actual";
        assert_eq!(
            decode_str_summaries(&fixture.build(), options("private-actual")),
            Err(StrSummaryDecodeError::AmbiguousSerial)
        );

        fixture.recording_id = "ResMed SRN= SRN=private-actual";
        assert_eq!(
            decode_str_summaries(&fixture.build(), options("private-actual"))
                .unwrap()
                .serial_verification,
            StrSummarySerialVerification::Verified
        );

        fixture.recording_id = "ResMed SRN= SRN=";
        assert_eq!(
            decode_str_summaries(&fixture.build(), options("private-actual")),
            Err(StrSummaryDecodeError::MissingSerial)
        );

        fixture.recording_id = "ResMed recording without marker";
        let missing = fixture.build();
        assert_eq!(
            decode_str_summaries(&missing, options("private-expected")),
            Err(StrSummaryDecodeError::MissingSerial)
        );
        assert_eq!(
            decode_str_summaries(&missing, options("")),
            Err(StrSummaryDecodeError::EmptyExpectedSerial)
        );
    }

    #[test]
    fn strict_header_contract_rejects_non_daily_non_noon_edf_plus_and_trailing_data() {
        let mut wrong_duration = EdfFixture::new(1, vec![SignalFixture::scalar("AHI", &[1])]);
        wrong_duration.duration = "3600";
        assert_eq!(
            decode_str_summaries(&wrong_duration.build(), options(SERIAL)),
            Err(StrSummaryDecodeError::InvalidRecordDuration)
        );

        let mut wrong_start = EdfFixture::new(1, vec![SignalFixture::scalar("AHI", &[1])]);
        wrong_start.start = "01.01.2611.59.59";
        assert_eq!(
            decode_str_summaries(&wrong_start.build(), options(SERIAL)),
            Err(StrSummaryDecodeError::InvalidRecordStart)
        );

        let mut edf_plus = EdfFixture::new(1, vec![SignalFixture::scalar("EDF Annotations", &[0])]);
        edf_plus.reserved = "EDF+C";
        assert_eq!(
            decode_str_summaries(&edf_plus.build(), options(SERIAL)),
            Err(StrSummaryDecodeError::UnsupportedEdfPlus)
        );

        let mut trailing = EdfFixture::new(1, vec![SignalFixture::scalar("AHI", &[1])]).build();
        trailing.extend_from_slice(&[0x12, 0x34]);
        assert_eq!(
            decode_str_summaries(&trailing, options(SERIAL)),
            Err(StrSummaryDecodeError::TrailingData { bytes: 2 })
        );
    }

    #[test]
    fn malformed_counts_and_truncation_fail_before_any_partial_output() {
        let valid = EdfFixture::new(1, vec![SignalFixture::scalar("AHI", &[1])]).build();

        let mut truncated = valid.clone();
        truncated.pop();
        let error = decode_str_summaries(&truncated, options(SERIAL)).unwrap_err();
        assert_eq!(
            error,
            StrSummaryDecodeError::Parse(StrSummaryParseFailure::DataLengthMismatch)
        );

        let mut private_header_value = valid.clone();
        const INJECTED_PRIVATE_VALUE: &str = "PHI777";
        overwrite_field(&mut private_header_value, 244, 8, INJECTED_PRIVATE_VALUE);
        let error = decode_str_summaries(&private_header_value, options(SERIAL)).unwrap_err();
        assert_eq!(
            error,
            StrSummaryDecodeError::Parse(StrSummaryParseFailure::InvalidNumber)
        );
        assert!(!format!("{error:?}").contains(INJECTED_PRIVATE_VALUE));
        assert!(!error.to_string().contains(INJECTED_PRIVATE_VALUE));

        let mut unknown = valid.clone();
        overwrite_field(&mut unknown, 236, 8, "-1");
        assert_eq!(
            decode_str_summaries(&unknown, options(SERIAL)),
            Err(StrSummaryDecodeError::UnknownRecordCount)
        );

        let mut empty = valid.clone();
        overwrite_field(&mut empty, 236, 8, "0");
        assert_eq!(
            decode_str_summaries(&empty, options(SERIAL)),
            Err(StrSummaryDecodeError::EmptyRecordSet)
        );

        let mut too_many_records = valid;
        overwrite_field(&mut too_many_records, 236, 8, "20001");
        assert_eq!(
            decode_str_summaries(&too_many_records, options(SERIAL)),
            Err(StrSummaryDecodeError::OutputLimitExceeded {
                limit: MAX_REPORTED_VALUES,
                actual: 20_001 * ALL_METRICS.len(),
            })
        );
    }

    #[test]
    fn input_byte_limit_is_enforced_before_header_parsing() {
        let bytes = vec![0_u8; RESMED_STR_SUMMARY_MAX_FILE_BYTES + 1];
        assert_eq!(
            decode_str_summaries(&bytes, options(SERIAL)),
            Err(StrSummaryDecodeError::FileTooLarge {
                limit: RESMED_STR_SUMMARY_MAX_FILE_BYTES,
                actual: RESMED_STR_SUMMARY_MAX_FILE_BYTES + 1,
            })
        );

        let too_many_signals = EdfFixture::new(
            1,
            (0..=MAX_SIGNALS)
                .map(|_| SignalFixture::scalar("unused", &[0]))
                .collect(),
        )
        .build();
        assert_eq!(
            decode_str_summaries(&too_many_signals, options(SERIAL)),
            Err(StrSummaryDecodeError::Parse(
                StrSummaryParseFailure::LimitExceeded
            ))
        );
    }
}
