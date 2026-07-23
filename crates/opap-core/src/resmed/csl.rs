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

//! Bounded decoding of ResMed CSL Cheyne-Stokes intervals.
//!
//! Pinned OSCAR `ResmedLoader::LoadCSL` walks annotations in record-major
//! order, recognizes the exact case-sensitive texts `CSR Start` and `CSR End`,
//! replaces an already-open start, and emits a duration only when an end has an
//! open start. This module preserves those observable pairing rules without
//! assigning the result to a session.
//!
//! OPAP deliberately adds strict input, identity, allocation, timestamp, and
//! duration bounds and requires the format's annotation-only signal shape.
//! Orphan ends, replaced starts, unfinished starts, and unknown texts are
//! represented only by typed aggregate counts; raw annotation text and machine
//! serials never enter diagnostics.

// Session routing is intentionally a later slice. Keep this complete decoder
// checked while its private parent module has no production caller yet.
#![allow(dead_code)]

use crate::{
    domain::DeviceLocalDateTime,
    resmed::{ResmedDeviceLocalTime, ResmedEdfHeaderSummary},
};
use opap_edf::{EdfFile, EdfHeader, Limits, ParseError, Parser};
use serde::{Deserialize, Serialize};
use std::{error, fmt};

const MAX_SIGNALS: usize = 256;
const MAX_RECORDS: usize = 65_536;
const MAX_SIGNAL_RECORDS: usize = 262_144;
const MAX_TOTAL_SAMPLES: usize = 8 * 1024 * 1024;
const MAX_ANNOTATION_RECORDS: usize = 262_144;
const MAX_ANNOTATIONS: usize = 262_144;
const MAX_ANNOTATION_TEXT_BYTES: usize = 4 * 1024 * 1024;

/// Largest complete uncompressed ResMed CSL file accepted by this decoder.
pub const RESMED_CSL_MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

/// Largest CSR interval accepted from one paired start and end.
pub const RESMED_CSL_MAX_SPAN_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

/// Largest recognized boundary offset from a day-wide CSL recording start.
pub const RESMED_CSL_MAX_OFFSET_MS: u64 = RESMED_CSL_MAX_SPAN_MS;

/// Largest number of CSR intervals materialized from one CSL source.
pub const RESMED_CSL_MAX_SPANS: usize = MAX_ANNOTATIONS / 2;

const CSL_LIMITS: Limits = Limits {
    max_signals: MAX_SIGNALS,
    max_records: MAX_RECORDS,
    max_signal_records: MAX_SIGNAL_RECORDS,
    max_total_samples: MAX_TOTAL_SAMPLES,
    max_annotation_bytes: RESMED_CSL_MAX_FILE_BYTES,
    max_annotation_records: MAX_ANNOTATION_RECORDS,
    max_annotations: MAX_ANNOTATIONS,
    max_annotation_text_bytes: MAX_ANNOTATION_TEXT_BYTES,
};

/// Caller-provided consistency expectations for one CSL source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CslDecodeOptions<'a> {
    /// Identification serial expected in the EDF `SRN=` recording token.
    ///
    /// Production import should provide this value. `None` explicitly records
    /// that the caller chose not to verify the file identity.
    pub expected_serial: Option<&'a str>,
    /// Header summary captured before the complete read, when available.
    ///
    /// Supplying this closes the header portion of an inventory/read TOCTOU
    /// boundary. `None` is explicit unverified mode for callers that have not
    /// yet indexed annotation-only files.
    pub expected_header: Option<&'a ResmedEdfHeaderSummary>,
}

/// Whether the decoder verified one input property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CslVerification {
    Verified,
    NotRequested,
}

/// Stable location of an annotation inside the source EDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CslAnnotationSource {
    pub record_index: u32,
    pub signal_index: u16,
    pub annotation_index: u32,
}

/// One half-open Cheyne-Stokes respiration interval, relative to the EDF start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CslSpan {
    /// Inclusive start offset from the device-local EDF start.
    pub start_offset_ms: u64,
    /// Exclusive end offset from the device-local EDF start.
    pub end_offset_ms: u64,
    /// Device-local inclusive start, without an implied UTC offset.
    pub start_time: DeviceLocalDateTime,
    /// Device-local exclusive end, without an implied UTC offset.
    pub end_time: DeviceLocalDateTime,
    pub start_source: CslAnnotationSource,
    pub end_source: CslAnnotationSource,
}

impl CslSpan {
    /// Duration of a decoder-produced span.
    ///
    /// Saturation keeps this helper total if an untrusted serialized value is
    /// inspected before a future persistence layer validates the invariant.
    #[must_use]
    pub const fn duration_ms(self) -> u64 {
        self.end_offset_ms.saturating_sub(self.start_offset_ms)
    }
}

/// Privacy-safe category for annotations that could not produce a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CslWarningKind {
    /// A second `CSR Start` replaced the previously open start.
    RepeatedStart,
    /// A `CSR End` had no open start.
    OrphanEnd,
    /// The file ended with an unpaired `CSR Start`.
    UnfinishedStart,
    /// A non-empty annotation other than the three known exact texts appeared.
    UnknownAnnotation,
}

impl CslWarningKind {
    /// Stable code for conversion to an import-layer warning.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::RepeatedStart => "resmed_csl_repeated_start",
            Self::OrphanEnd => "resmed_csl_orphan_end",
            Self::UnfinishedStart => "resmed_csl_unfinished_start",
            Self::UnknownAnnotation => "unknown_resmed_csl_annotation",
        }
    }

    /// Privacy-safe message that never includes source text or identity.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::RepeatedStart => "A repeated CSL CSR start replaced the previously open boundary",
            Self::OrphanEnd => "A CSL CSR end without an open start was ignored",
            Self::UnfinishedStart => "An unfinished CSL CSR start was ignored at end of file",
            Self::UnknownAnnotation => "An unsupported CSL annotation was ignored",
        }
    }
}

/// Aggregated warning count. No source text, path, or identity is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CslWarning {
    pub kind: CslWarningKind,
    pub count: u32,
}

/// Complete bounded output for one CSL file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CslSpanIndex {
    /// Device-local start encoded by the EDF header after ResMed's century
    /// repair for wire years 85 through 99.
    pub recording_start: DeviceLocalDateTime,
    pub serial_verification: CslVerification,
    pub header_verification: CslVerification,
    /// Spans remain in source traversal order and use half-open boundaries.
    pub spans: Vec<CslSpan>,
    /// At most one entry per [`CslWarningKind`], in stable enum order.
    pub warnings: Vec<CslWarning>,
}

/// Which endpoint failed timestamp validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CslBoundaryRole {
    Start,
    End,
}

impl fmt::Display for CslBoundaryRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Start => "start",
            Self::End => "end",
        })
    }
}

/// Failure to produce a trustworthy bounded CSL span index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CslDecodeError {
    FileTooLarge {
        limit: usize,
        actual: usize,
    },
    EmptyExpectedSerial,
    Parse(ParseError),
    TrailingData {
        bytes: usize,
    },
    HeaderMismatch,
    MissingSerial,
    SerialMismatch,
    InvalidRecordingStart,
    MissingAnnotationSignal,
    NonAnnotationSignal {
        signal_index: usize,
    },
    EmptyAnnotationRecord {
        signal_index: usize,
    },
    TimestampOutOfRange {
        boundary: CslBoundaryRole,
    },
    DateRange {
        boundary: CslBoundaryRole,
    },
    NonPositiveSpan,
    SpanTooLong {
        limit_ms: u64,
    },
    SpanLimitExceeded {
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl fmt::Display for CslDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { limit, actual } => {
                write!(
                    formatter,
                    "CSL EDF exceeds the {limit}-byte input limit ({actual} bytes)"
                )
            }
            Self::EmptyExpectedSerial => {
                formatter.write_str("expected ResMed serial must not be empty")
            }
            Self::Parse(source) => write!(formatter, "could not parse bounded CSL EDF: {source}"),
            Self::TrailingData { bytes } => {
                write!(
                    formatter,
                    "CSL EDF has {bytes} trailing bytes after its declared records"
                )
            }
            Self::HeaderMismatch => {
                formatter.write_str("complete CSL header does not match its indexed header summary")
            }
            Self::MissingSerial => {
                formatter.write_str("CSL recording identity is missing its SRN token")
            }
            Self::SerialMismatch => formatter
                .write_str("CSL recording identity does not match the selected ResMed card"),
            Self::InvalidRecordingStart => {
                formatter.write_str("CSL recording start is outside the supported calendar range")
            }
            Self::MissingAnnotationSignal => {
                formatter.write_str("CSL EDF contains no compatible annotation signal")
            }
            Self::NonAnnotationSignal { signal_index } => write!(
                formatter,
                "CSL EDF signal {signal_index} is sampled data, not annotations"
            ),
            Self::EmptyAnnotationRecord { signal_index } => write!(
                formatter,
                "CSL EDF annotation signal {signal_index} has an empty record layout"
            ),
            Self::TimestampOutOfRange { boundary } => {
                write!(
                    formatter,
                    "CSL CSR {boundary} timestamp is negative or outside the supported range"
                )
            }
            Self::DateRange { boundary } => {
                write!(
                    formatter,
                    "CSL CSR {boundary} timestamp exceeds the supported calendar range"
                )
            }
            Self::NonPositiveSpan => {
                formatter.write_str("CSL CSR end must be later than its paired start")
            }
            Self::SpanTooLong { limit_ms } => {
                write!(
                    formatter,
                    "CSL CSR span exceeds the {limit_ms}-millisecond duration limit"
                )
            }
            Self::SpanLimitExceeded { limit } => {
                write!(
                    formatter,
                    "CSL CSR span count exceeds the {limit}-span output limit"
                )
            }
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not reserve capacity for {requested} {resource}"
            ),
        }
    }
}

impl error::Error for CslDecodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Parse(source) => Some(source),
            _ => None,
        }
    }
}

impl From<ParseError> for CslDecodeError {
    fn from(source: ParseError) -> Self {
        Self::Parse(source)
    }
}

/// Decode Cheyne-Stokes intervals from one complete, uncompressed CSL EDF.
///
/// Pairs are processed in EDF record order, then signal order, then annotation
/// order, matching the pinned OSCAR parser's flattened annotation traversal.
/// `CSR Start` and `CSR End` are exact and case-sensitive. `Recording starts`
/// is the only exact non-event text ignored without a warning.
///
/// # Errors
///
/// Returns [`CslDecodeError`] for an over-limit or malformed file, failed
/// identity/header consistency, unsafe timestamp, invalid pair, or output
/// resource exhaustion.
pub fn decode_csl(
    bytes: &[u8],
    options: CslDecodeOptions<'_>,
) -> Result<CslSpanIndex, CslDecodeError> {
    decode_csl_with_limits(
        bytes,
        options,
        CSL_LIMITS,
        RESMED_CSL_MAX_FILE_BYTES,
        RESMED_CSL_MAX_SPANS,
    )
}

fn decode_csl_with_limits(
    bytes: &[u8],
    options: CslDecodeOptions<'_>,
    parser_limits: Limits,
    file_byte_limit: usize,
    span_limit: usize,
) -> Result<CslSpanIndex, CslDecodeError> {
    if bytes.len() > file_byte_limit {
        return Err(CslDecodeError::FileTooLarge {
            limit: file_byte_limit,
            actual: bytes.len(),
        });
    }
    if options.expected_serial == Some("") {
        return Err(CslDecodeError::EmptyExpectedSerial);
    }

    let parser = Parser::new(parser_limits);
    let header = parser.parse_header(bytes)?;
    validate_header_shape(&header)?;
    let recording_start = resmed_recording_start(&header)?;
    let recording_start_ms =
        local_millis(&recording_start).ok_or(CslDecodeError::InvalidRecordingStart)?;
    let header_verification = verify_header(&header, &recording_start, options.expected_header)?;
    let serial_verification = verify_serial(&header.recording_id, options.expected_serial)?;

    let parsed = parser.parse(bytes)?;
    if parsed.trailing_data_bytes() != 0 {
        return Err(CslDecodeError::TrailingData {
            bytes: parsed.trailing_data_bytes(),
        });
    }
    decode_annotations(
        &parsed,
        recording_start,
        recording_start_ms,
        serial_verification,
        header_verification,
        span_limit,
    )
}

fn decode_annotations(
    parsed: &EdfFile,
    recording_start: DeviceLocalDateTime,
    recording_start_ms: i64,
    serial_verification: CslVerification,
    header_verification: CslVerification,
    span_limit: usize,
) -> Result<CslSpanIndex, CslDecodeError> {
    let mut spans = Vec::new();
    let mut open_start = None;
    let mut diagnostics = WarningCounts::default();

    // OSCAR's EDF parser appends one annotation vector per annotation signal
    // while walking records first and signals second. Reconstruct that same
    // ordering from opap-edf's per-signal representation.
    for record in parsed.records() {
        for (signal_index, signal) in parsed.signals().iter().enumerate() {
            let Some(annotations) = record.annotations(signal_index) else {
                debug_assert!(signal.annotation_records().is_none());
                continue;
            };
            for (annotation_index, annotation) in annotations.iter().enumerate() {
                let source = CslAnnotationSource {
                    record_index: u32::try_from(record.index()).expect("CSL record limit fits u32"),
                    signal_index: u16::try_from(signal_index).expect("CSL signal limit fits u16"),
                    annotation_index: u32::try_from(annotation_index)
                        .expect("CSL annotation limit fits u32"),
                };
                match annotation.text.as_str() {
                    "CSR Start" => {
                        let start_offset_ms =
                            timestamp_millis(annotation.onset_seconds, CslBoundaryRole::Start)?;
                        if open_start.is_some() {
                            diagnostics.repeated_start =
                                diagnostics.repeated_start.saturating_add(1);
                        }
                        open_start = Some(OpenStart {
                            offset_ms: start_offset_ms,
                            source,
                            onset_seconds: annotation.onset_seconds,
                        });
                    }
                    "CSR End" => {
                        if open_start.is_some_and(|start| {
                            annotation.onset_seconds - start.onset_seconds
                                > RESMED_CSL_MAX_SPAN_MS as f64 / 1_000.0
                        }) {
                            return Err(CslDecodeError::SpanTooLong {
                                limit_ms: RESMED_CSL_MAX_SPAN_MS,
                            });
                        }
                        let end_offset_ms =
                            timestamp_millis(annotation.onset_seconds, CslBoundaryRole::End)?;
                        let Some(start) = open_start.take() else {
                            diagnostics.orphan_end = diagnostics.orphan_end.saturating_add(1);
                            continue;
                        };
                        if annotation.onset_seconds <= start.onset_seconds
                            || end_offset_ms <= start.offset_ms
                        {
                            return Err(CslDecodeError::NonPositiveSpan);
                        }
                        let duration_ms = end_offset_ms - start.offset_ms;
                        if duration_ms > RESMED_CSL_MAX_SPAN_MS {
                            return Err(CslDecodeError::SpanTooLong {
                                limit_ms: RESMED_CSL_MAX_SPAN_MS,
                            });
                        }
                        if spans.len() >= span_limit {
                            return Err(CslDecodeError::SpanLimitExceeded { limit: span_limit });
                        }
                        let start_time = boundary_time(
                            recording_start_ms,
                            start.offset_ms,
                            CslBoundaryRole::Start,
                        )?;
                        let end_time =
                            boundary_time(recording_start_ms, end_offset_ms, CslBoundaryRole::End)?;
                        spans
                            .try_reserve(1)
                            .map_err(|_| CslDecodeError::AllocationFailed {
                                resource: "CSL CSR spans",
                                requested: spans.len().saturating_add(1),
                            })?;
                        spans.push(CslSpan {
                            start_offset_ms: start.offset_ms,
                            end_offset_ms,
                            start_time,
                            end_time,
                            start_source: start.source,
                            end_source: source,
                        });
                    }
                    "Recording starts" => {}
                    _ => {
                        diagnostics.unknown_annotation =
                            diagnostics.unknown_annotation.saturating_add(1);
                    }
                }
            }
        }
    }

    if open_start.is_some() {
        diagnostics.unfinished_start = diagnostics.unfinished_start.saturating_add(1);
    }
    let warnings = diagnostics.into_warnings()?;
    Ok(CslSpanIndex {
        recording_start,
        serial_verification,
        header_verification,
        spans,
        warnings,
    })
}

#[derive(Debug, Clone, Copy)]
struct OpenStart {
    offset_ms: u64,
    source: CslAnnotationSource,
    onset_seconds: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct WarningCounts {
    repeated_start: u32,
    orphan_end: u32,
    unfinished_start: u32,
    unknown_annotation: u32,
}

impl WarningCounts {
    fn into_warnings(self) -> Result<Vec<CslWarning>, CslDecodeError> {
        let mut warnings = Vec::new();
        warnings
            .try_reserve_exact(4)
            .map_err(|_| CslDecodeError::AllocationFailed {
                resource: "CSL warnings",
                requested: 4,
            })?;
        for (kind, count) in [
            (CslWarningKind::RepeatedStart, self.repeated_start),
            (CslWarningKind::OrphanEnd, self.orphan_end),
            (CslWarningKind::UnfinishedStart, self.unfinished_start),
            (CslWarningKind::UnknownAnnotation, self.unknown_annotation),
        ] {
            if count != 0 {
                warnings.push(CslWarning { kind, count });
            }
        }
        Ok(warnings)
    }
}

fn validate_header_shape(header: &EdfHeader) -> Result<(), CslDecodeError> {
    if header.signals.is_empty() {
        return Err(CslDecodeError::MissingAnnotationSignal);
    }
    for (signal_index, signal) in header.signals.iter().enumerate() {
        if !signal.is_annotation_signal() {
            return Err(CslDecodeError::NonAnnotationSignal { signal_index });
        }
        if signal.samples_per_record == 0 {
            return Err(CslDecodeError::EmptyAnnotationRecord { signal_index });
        }
    }
    Ok(())
}

fn timestamp_millis(onset_seconds: f64, boundary: CslBoundaryRole) -> Result<u64, CslDecodeError> {
    let milliseconds = onset_seconds * 1_000.0;
    if !onset_seconds.is_finite()
        || onset_seconds < 0.0
        || !milliseconds.is_finite()
        || milliseconds > RESMED_CSL_MAX_OFFSET_MS as f64
    {
        return Err(CslDecodeError::TimestampOutOfRange { boundary });
    }

    // The pinned loader converts `offset * 1000` to qint64, truncating positive
    // sub-millisecond fractions toward zero. Validation above makes the cast
    // explicit and safe on every target.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(milliseconds as u64)
}

fn boundary_time(
    recording_start_ms: i64,
    offset_ms: u64,
    boundary: CslBoundaryRole,
) -> Result<DeviceLocalDateTime, CslDecodeError> {
    let offset_ms = i64::try_from(offset_ms).map_err(|_| CslDecodeError::DateRange { boundary })?;
    let value = recording_start_ms
        .checked_add(offset_ms)
        .ok_or(CslDecodeError::DateRange { boundary })?;
    local_datetime_from_millis(value).ok_or(CslDecodeError::DateRange { boundary })
}

fn verify_serial(
    recording_id: &str,
    expected_serial: Option<&str>,
) -> Result<CslVerification, CslDecodeError> {
    let Some(expected) = expected_serial else {
        return Ok(CslVerification::NotRequested);
    };
    match recording_serial(recording_id) {
        Some(actual) if actual == expected => Ok(CslVerification::Verified),
        Some(_) => Err(CslDecodeError::SerialMismatch),
        None => Err(CslDecodeError::MissingSerial),
    }
}

fn recording_serial(recording_id: &str) -> Option<&str> {
    recording_id
        .split_ascii_whitespace()
        .filter_map(|token| token.strip_prefix("SRN="))
        .find(|serial| !serial.is_empty())
}

fn verify_header(
    header: &EdfHeader,
    recording_start: &DeviceLocalDateTime,
    expected: Option<&ResmedEdfHeaderSummary>,
) -> Result<CslVerification, CslDecodeError> {
    let Some(expected) = expected else {
        return Ok(CslVerification::NotRequested);
    };
    let structural_match = u64::try_from(header.header_bytes).ok() == Some(expected.header_bytes)
        && u16::try_from(header.signals.len()).ok() == Some(expected.signal_count)
        && header
            .declared_record_count
            .and_then(|count| u64::try_from(count).ok())
            == expected.declared_record_count
        && header.record_duration_seconds.to_bits() == expected.record_duration_seconds.to_bits();
    let start_match = expected
        .start_time
        .as_ref()
        .is_none_or(|start| summary_start_matches(start, recording_start));
    if structural_match && start_match {
        Ok(CslVerification::Verified)
    } else {
        Err(CslDecodeError::HeaderMismatch)
    }
}

fn summary_start_matches(expected: &ResmedDeviceLocalTime, actual: &DeviceLocalDateTime) -> bool {
    let canonical_wall_time = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        expected.year,
        expected.month,
        expected.day,
        expected.hour,
        expected.minute,
        expected.second
    );
    expected.wall_time == canonical_wall_time
        && expected.year == actual.year
        && expected.month == actual.month
        && expected.day == actual.day
        && expected.hour == actual.hour
        && expected.minute == actual.minute
        && expected.second == actual.second
        && expected.millisecond == actual.millisecond
}

fn resmed_recording_start(header: &EdfHeader) -> Result<DeviceLocalDateTime, CslDecodeError> {
    // Pinned ResMed code repairs the shared EDF parser's 1985..1999 pivot into
    // 2085..2099. Preserve that source-specific clock interpretation.
    let year = if header.start.year < 2_000 {
        header
            .start
            .year
            .checked_add(100)
            .ok_or(CslDecodeError::InvalidRecordingStart)?
    } else {
        header.start.year
    };
    let start = DeviceLocalDateTime {
        year,
        month: header.start.month,
        day: header.start.day,
        hour: header.start.hour,
        minute: header.start.minute,
        second: header.start.second,
        millisecond: 0,
    };
    valid_local_datetime(&start)
        .then_some(start)
        .ok_or(CslDecodeError::InvalidRecordingStart)
}

fn valid_local_datetime(value: &DeviceLocalDateTime) -> bool {
    value.year > 0
        && (1..=12).contains(&value.month)
        && value.day > 0
        && value.day <= days_in_month(value.year, value.month)
        && value.hour < 24
        && value.minute < 60
        && value.second < 60
        && value.millisecond < 1_000
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    }
}

fn local_millis(value: &DeviceLocalDateTime) -> Option<i64> {
    days_from_civil(value.year, value.month, value.day)
        .checked_mul(86_400_000)?
        .checked_add(i64::from(value.hour) * 3_600_000)?
        .checked_add(i64::from(value.minute) * 60_000)?
        .checked_add(i64::from(value.second) * 1_000)?
        .checked_add(i64::from(value.millisecond))
}

fn local_datetime_from_millis(value: i64) -> Option<DeviceLocalDateTime> {
    let days = value.div_euclid(86_400_000);
    let within_day = value.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days)?;
    Some(DeviceLocalDateTime {
        year,
        month,
        day,
        hour: u8::try_from(within_day / 3_600_000).ok()?,
        minute: u8::try_from((within_day % 3_600_000) / 60_000).ok()?,
        second: u8::try_from((within_day % 60_000) / 1_000).ok()?,
        millisecond: u16::try_from(within_day % 1_000).ok()?,
    })
}

// Howard Hinnant's civil-calendar conversion, adjusted to the Unix epoch.
fn days_from_civil(year: u16, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> Option<(u16, u8, u8)> {
    let shifted = days.checked_add(719_468)?;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
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
    use opap_edf::ParseErrorKind;

    #[derive(Clone)]
    struct SignalFixture<'a> {
        label: &'a str,
        records: Vec<Vec<u8>>,
    }

    impl<'a> SignalFixture<'a> {
        fn annotations(label: &'a str, records: Vec<Vec<u8>>) -> Self {
            Self { label, records }
        }

        fn digital(label: &'a str, record_count: usize) -> Self {
            Self {
                label,
                records: vec![vec![0, 0]; record_count],
            }
        }
    }

    fn field(value: &str, width: usize) -> Vec<u8> {
        assert!(value.len() <= width);
        let mut output = vec![b' '; width];
        output[..value.len()].copy_from_slice(value.as_bytes());
        output
    }

    fn tal(onset: &str, text: &str) -> Vec<u8> {
        format!("{onset}\u{14}{text}\u{14}\0").into_bytes()
    }

    fn tal_with_duration(onset: &str, duration: &str, text: &str) -> Vec<u8> {
        format!("{onset}\u{15}{duration}\u{14}{text}\u{14}\0").into_bytes()
    }

    fn annotation_record(entries: &[(&str, &str)]) -> Vec<u8> {
        entries
            .iter()
            .flat_map(|(onset, text)| tal(onset, text))
            .collect()
    }

    fn synthetic_csl(
        signals: &[SignalFixture<'_>],
        recording_id: &str,
        start: &str,
        record_duration: &str,
        reserved: &str,
    ) -> Vec<u8> {
        assert!(!signals.is_empty());
        assert_eq!(start.len(), 16);
        let record_count = signals[0].records.len();
        assert!(
            signals
                .iter()
                .all(|signal| signal.records.len() == record_count)
        );
        let widths: Vec<_> = signals
            .iter()
            .map(|signal| {
                let maximum = signal
                    .records
                    .iter()
                    .map(Vec::len)
                    .max()
                    .unwrap_or(0)
                    .max(2);
                maximum.next_multiple_of(2)
            })
            .collect();
        let header_bytes = 256 + signals.len() * 256;
        let mut bytes = Vec::new();
        bytes.extend(field("0", 8));
        bytes.extend(field("private-patient", 80));
        bytes.extend(field(recording_id, 80));
        bytes.extend_from_slice(start.as_bytes());
        bytes.extend(field(&header_bytes.to_string(), 8));
        bytes.extend(field(reserved, 44));
        bytes.extend(field(&record_count.to_string(), 8));
        bytes.extend(field(record_duration, 8));
        bytes.extend(field(&signals.len().to_string(), 4));

        for signal in signals {
            bytes.extend(field(signal.label, 16));
        }
        for _ in signals {
            bytes.extend(field("", 80));
        }
        for _ in signals {
            bytes.extend(field("raw", 8));
        }
        for _ in signals {
            bytes.extend(field("-32768", 8));
        }
        for _ in signals {
            bytes.extend(field("32767", 8));
        }
        for _ in signals {
            bytes.extend(field("-32768", 8));
        }
        for _ in signals {
            bytes.extend(field("32767", 8));
        }
        for _ in signals {
            bytes.extend(field("", 80));
        }
        for width in &widths {
            bytes.extend(field(&(width / 2).to_string(), 8));
        }
        for _ in signals {
            bytes.extend(field("", 32));
        }
        assert_eq!(bytes.len(), header_bytes);

        for record_index in 0..record_count {
            for (signal, width) in signals.iter().zip(&widths) {
                let record = &signal.records[record_index];
                bytes.extend_from_slice(record);
                bytes.resize(bytes.len() + (width - record.len()), 0);
            }
        }
        bytes
    }

    fn standard_csl(records: Vec<Vec<u8>>) -> Vec<u8> {
        synthetic_csl(
            &[SignalFixture::annotations("EDF Annotations", records)],
            "ResMed SRN=serial-123",
            "01.01.2612.00.00",
            "1",
            "",
        )
    }

    fn indexed_header(bytes: &[u8]) -> ResmedEdfHeaderSummary {
        let header = Parser::default()
            .parse_header(bytes)
            .expect("fixture header");
        let year = if header.start.year < 2_000 {
            header.start.year + 100
        } else {
            header.start.year
        };
        ResmedEdfHeaderSummary {
            start_time: Some(ResmedDeviceLocalTime {
                wall_time: format!(
                    "{year:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                    header.start.month,
                    header.start.day,
                    header.start.hour,
                    header.start.minute,
                    header.start.second
                ),
                year,
                month: header.start.month,
                day: header.start.day,
                hour: header.start.hour,
                minute: header.start.minute,
                second: header.start.second,
                millisecond: 0,
            }),
            header_bytes: u64::try_from(header.header_bytes).unwrap(),
            signal_count: u16::try_from(header.signals.len()).unwrap(),
            declared_record_count: header
                .declared_record_count
                .map(|count| u64::try_from(count).unwrap()),
            record_duration_seconds: header.record_duration_seconds,
            estimated_duration_millis: None,
        }
    }

    fn options<'a>(
        expected_serial: Option<&'a str>,
        expected_header: Option<&'a ResmedEdfHeaderSummary>,
    ) -> CslDecodeOptions<'a> {
        CslDecodeOptions {
            expected_serial,
            expected_header,
        }
    }

    fn warning(kind: CslWarningKind, count: u32) -> CslWarning {
        CslWarning { kind, count }
    }

    #[test]
    fn pinned_oscar_pairs_exact_boundaries_across_records() {
        let mut first = tal_with_duration("+1.2509", "99", "CSR Start");
        first.extend(tal("+0", "Recording starts"));
        let bytes = standard_csl(vec![first, tal("+20", "CSR End")]);
        let header = indexed_header(&bytes);

        let decoded =
            decode_csl(&bytes, options(Some("serial-123"), Some(&header))).expect("valid CSL");

        assert_eq!(decoded.serial_verification, CslVerification::Verified);
        assert_eq!(decoded.header_verification, CslVerification::Verified);
        assert_eq!(
            decoded.recording_start,
            DeviceLocalDateTime {
                year: 2026,
                month: 1,
                day: 1,
                hour: 12,
                minute: 0,
                second: 0,
                millisecond: 0,
            }
        );
        assert_eq!(
            decoded.spans,
            vec![CslSpan {
                start_offset_ms: 1_250,
                end_offset_ms: 20_000,
                start_time: DeviceLocalDateTime {
                    year: 2026,
                    month: 1,
                    day: 1,
                    hour: 12,
                    minute: 0,
                    second: 1,
                    millisecond: 250,
                },
                end_time: DeviceLocalDateTime {
                    year: 2026,
                    month: 1,
                    day: 1,
                    hour: 12,
                    minute: 0,
                    second: 20,
                    millisecond: 0,
                },
                start_source: CslAnnotationSource {
                    record_index: 0,
                    signal_index: 0,
                    annotation_index: 0,
                },
                end_source: CslAnnotationSource {
                    record_index: 1,
                    signal_index: 0,
                    annotation_index: 0,
                },
            }]
        );
        assert_eq!(decoded.spans[0].duration_ms(), 18_750);
        assert!(decoded.warnings.is_empty());
    }

    #[test]
    fn preserves_record_then_signal_then_annotation_order() {
        let bytes = synthetic_csl(
            &[
                SignalFixture::annotations(
                    "A Annotations",
                    vec![tal("+1", "CSR Start"), tal("+3", "CSR Start")],
                ),
                SignalFixture::annotations(
                    "B Annotations",
                    vec![tal("+2", "CSR End"), tal("+4", "CSR End")],
                ),
            ],
            "SRN=serial-123",
            "01.01.2612.00.00",
            "1",
            "",
        );

        let decoded = decode_csl(&bytes, options(Some("serial-123"), None)).unwrap();

        assert_eq!(
            decoded
                .spans
                .iter()
                .map(|span| (span.start_offset_ms, span.end_offset_ms))
                .collect::<Vec<_>>(),
            vec![(1_000, 2_000), (3_000, 4_000)]
        );
        assert!(decoded.warnings.is_empty());
    }

    #[test]
    fn repeated_orphan_unfinished_and_unknown_annotations_are_aggregated() {
        let bytes = standard_csl(vec![
            annotation_record(&[
                ("+0", "CSR End"),
                ("+1", "CSR Start"),
                ("+2", "CSR Start"),
                ("+3", "CSR End"),
                ("+4", "csr start"),
                ("+5", "CSR START"),
                ("+6", "private-patient-secret"),
                ("+7", "Recording Starts"),
                ("+8", "Recording starts"),
                ("+9", "CSR Start"),
            ]),
            tal("+10", "CSR Start"),
        ]);

        let decoded = decode_csl(&bytes, options(Some("serial-123"), None)).unwrap();

        assert_eq!(
            decoded
                .spans
                .iter()
                .map(|span| (span.start_offset_ms, span.end_offset_ms))
                .collect::<Vec<_>>(),
            vec![(2_000, 3_000)]
        );
        assert_eq!(
            decoded.warnings,
            vec![
                warning(CslWarningKind::RepeatedStart, 2),
                warning(CslWarningKind::OrphanEnd, 1),
                warning(CslWarningKind::UnfinishedStart, 1),
                warning(CslWarningKind::UnknownAnnotation, 4),
            ]
        );
        let diagnostic = format!("{:?}", decoded.warnings);
        assert!(!diagnostic.contains("private-patient-secret"));
        assert!(!diagnostic.contains("serial-123"));
        assert_eq!(
            CslWarningKind::RepeatedStart.code(),
            "resmed_csl_repeated_start"
        );
        assert!(
            CslWarningKind::UnknownAnnotation
                .message()
                .contains("unsupported CSL annotation")
        );
    }

    #[test]
    fn serial_verification_is_exact_private_and_explicitly_optional() {
        let missing = synthetic_csl(
            &[SignalFixture::annotations(
                "EDF Annotations",
                vec![Vec::new()],
            )],
            "ResMed recording",
            "01.01.2612.00.00",
            "1",
            "",
        );
        let mismatch = synthetic_csl(
            &[SignalFixture::annotations(
                "EDF Annotations",
                vec![Vec::new()],
            )],
            "ResMed SRN=actual-private-serial",
            "01.01.2612.00.00",
            "1",
            "",
        );

        assert_eq!(
            decode_csl(&missing, options(Some("expected-private-serial"), None)),
            Err(CslDecodeError::MissingSerial)
        );
        let mismatch_error =
            decode_csl(&mismatch, options(Some("expected-private-serial"), None)).unwrap_err();
        assert_eq!(mismatch_error, CslDecodeError::SerialMismatch);
        let diagnostic = format!("{mismatch_error:?} {mismatch_error}");
        assert!(!diagnostic.contains("expected-private-serial"));
        assert!(!diagnostic.contains("actual-private-serial"));

        let unverified = decode_csl(&missing, options(None, None)).unwrap();
        assert_eq!(
            unverified.serial_verification,
            CslVerification::NotRequested
        );
        assert_eq!(
            decode_csl(&missing, options(Some(""), None)),
            Err(CslDecodeError::EmptyExpectedSerial)
        );
    }

    #[test]
    fn indexed_header_is_checked_exactly_without_exposing_source_fields() {
        let bytes = standard_csl(vec![Vec::new()]);
        let matching = indexed_header(&bytes);
        assert_eq!(
            decode_csl(&bytes, options(Some("serial-123"), Some(&matching)))
                .unwrap()
                .header_verification,
            CslVerification::Verified
        );

        let mut changed_count = matching.clone();
        changed_count.declared_record_count = Some(2);
        assert_eq!(
            decode_csl(&bytes, options(Some("serial-123"), Some(&changed_count))),
            Err(CslDecodeError::HeaderMismatch)
        );

        let mut noncanonical_start = matching;
        noncanonical_start.start_time.as_mut().unwrap().wall_time =
            "private-header-value".to_owned();
        let error = decode_csl(
            &bytes,
            options(Some("serial-123"), Some(&noncanonical_start)),
        )
        .unwrap_err();
        assert_eq!(error, CslDecodeError::HeaderMismatch);
        assert!(!error.to_string().contains("private-header-value"));
    }

    #[test]
    fn resmed_century_repair_is_applied_to_the_recording_start_and_header_check() {
        let bytes = synthetic_csl(
            &[SignalFixture::annotations(
                "EDF Annotations",
                vec![Vec::new()],
            )],
            "SRN=serial-123",
            "01.01.8512.00.00",
            "1",
            "",
        );
        let header = indexed_header(&bytes);

        let decoded = decode_csl(&bytes, options(Some("serial-123"), Some(&header))).unwrap();
        assert_eq!(decoded.recording_start.year, 2085);
        assert_eq!(decoded.header_verification, CslVerification::Verified);
    }

    #[test]
    fn recognized_timestamps_and_spans_are_strictly_bounded() {
        let negative = standard_csl(vec![tal("-0.001", "CSR Start")]);
        assert_eq!(
            decode_csl(&negative, options(Some("serial-123"), None)),
            Err(CslDecodeError::TimestampOutOfRange {
                boundary: CslBoundaryRole::Start,
            })
        );

        let huge = standard_csl(vec![tal("+999999999999999999999", "CSR End")]);
        assert_eq!(
            decode_csl(&huge, options(Some("serial-123"), None)),
            Err(CslDecodeError::TimestampOutOfRange {
                boundary: CslBoundaryRole::End,
            })
        );

        for end in ["+1", "+0.0009"] {
            let nonpositive = standard_csl(vec![annotation_record(&[
                ("+1", "CSR Start"),
                (end, "CSR End"),
            ])]);
            assert_eq!(
                decode_csl(&nonpositive, options(Some("serial-123"), None)),
                Err(CslDecodeError::NonPositiveSpan)
            );
        }

        let too_long = standard_csl(vec![annotation_record(&[
            ("+0", "CSR Start"),
            ("+604800.001", "CSR End"),
        ])]);
        assert_eq!(
            decode_csl(&too_long, options(Some("serial-123"), None)),
            Err(CslDecodeError::SpanTooLong {
                limit_ms: RESMED_CSL_MAX_SPAN_MS,
            })
        );

        let exact = standard_csl(vec![annotation_record(&[
            ("+0", "CSR Start"),
            ("+604800", "CSR End"),
        ])]);
        let span = decode_csl(&exact, options(Some("serial-123"), None))
            .unwrap()
            .spans[0];
        assert_eq!(span.duration_ms(), RESMED_CSL_MAX_SPAN_MS);
        assert_eq!(
            span.end_time,
            DeviceLocalDateTime {
                year: 2026,
                month: 1,
                day: 8,
                hour: 12,
                minute: 0,
                second: 0,
                millisecond: 0,
            }
        );
    }

    #[test]
    fn malformed_shape_tal_trailing_data_and_non_annotation_input_fail_closed() {
        assert!(matches!(
            decode_csl(b"not an EDF", options(None, None)),
            Err(CslDecodeError::Parse(_))
        ));

        let mut malformed_tal = standard_csl(vec![tal("+1", "CSR Start")]);
        let header_bytes = Parser::default()
            .parse_header(&malformed_tal)
            .unwrap()
            .header_bytes;
        malformed_tal[header_bytes..].fill(b'x');
        assert!(matches!(
            decode_csl(&malformed_tal, options(None, None)),
            Err(CslDecodeError::Parse(ParseError {
                kind: ParseErrorKind::MalformedAnnotation { .. },
                ..
            }))
        ));

        let mut trailing = standard_csl(vec![Vec::new()]);
        trailing.extend_from_slice(b"private-trailing-data");
        assert_eq!(
            decode_csl(&trailing, options(None, None)),
            Err(CslDecodeError::TrailingData {
                bytes: "private-trailing-data".len(),
            })
        );

        let digital = synthetic_csl(
            &[SignalFixture::digital("Flow", 1)],
            "SRN=serial-123",
            "01.01.2612.00.00",
            "1",
            "",
        );
        assert_eq!(
            decode_csl(&digital, options(Some("serial-123"), None)),
            Err(CslDecodeError::NonAnnotationSignal { signal_index: 0 })
        );

        let mut empty_annotation_record = standard_csl(vec![Vec::new()]);
        empty_annotation_record[472..480].copy_from_slice(&field("0", 8));
        assert_eq!(
            decode_csl(&empty_annotation_record, options(None, None)),
            Err(CslDecodeError::EmptyAnnotationRecord { signal_index: 0 })
        );
    }

    #[test]
    fn file_parser_and_output_resource_limits_are_enforced() {
        let oversized = vec![0; RESMED_CSL_MAX_FILE_BYTES + 1];
        assert_eq!(
            decode_csl(&oversized, options(None, None)),
            Err(CslDecodeError::FileTooLarge {
                limit: RESMED_CSL_MAX_FILE_BYTES,
                actual: RESMED_CSL_MAX_FILE_BYTES + 1,
            })
        );

        let two_annotations = standard_csl(vec![annotation_record(&[
            ("+1", "CSR Start"),
            ("+2", "CSR End"),
        ])]);
        let one_annotation_limit = Limits {
            max_annotations: 1,
            ..CSL_LIMITS
        };
        assert!(matches!(
            decode_csl_with_limits(
                &two_annotations,
                options(Some("serial-123"), None),
                one_annotation_limit,
                RESMED_CSL_MAX_FILE_BYTES,
                RESMED_CSL_MAX_SPANS,
            ),
            Err(CslDecodeError::Parse(ParseError {
                kind: ParseErrorKind::LimitExceeded {
                    resource: "annotations",
                    limit: 1,
                    actual: 2,
                },
                ..
            }))
        ));

        let two_spans = standard_csl(vec![annotation_record(&[
            ("+1", "CSR Start"),
            ("+2", "CSR End"),
            ("+3", "CSR Start"),
            ("+4", "CSR End"),
        ])]);
        assert_eq!(
            decode_csl_with_limits(
                &two_spans,
                options(Some("serial-123"), None),
                CSL_LIMITS,
                RESMED_CSL_MAX_FILE_BYTES,
                1,
            ),
            Err(CslDecodeError::SpanLimitExceeded { limit: 1 })
        );
    }
}
