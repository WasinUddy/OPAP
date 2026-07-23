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

//! Bounded decoding of ResMed EVE event annotations.
//!
//! This module deliberately stops before session ownership. ResMed EVE files
//! commonly cover a whole device-local day, so assigning an annotation to a
//! therapy session is a separate operation that must use the session boundary
//! index. The decoder preserves source record order and the distinction
//! between an omitted duration and an explicit zero duration.
//!
//! Duration storage intentionally differs from pinned OSCAR. OSCAR narrows an
//! annotation duration to signed 16-bit whole seconds and uses `-1` as its
//! missing sentinel. OPAP's domain stores optional unsigned milliseconds, so
//! this decoder maps missing to `None`, preserves explicit zero as `Some(0)`,
//! and truncates positive fractional milliseconds toward zero.

// Session routing is intentionally a later slice. Keep this complete decoder
// checked while its private parent module has no production caller yet.
#![allow(dead_code)]

use super::{ResmedDeviceLocalTime, ResmedEdfHeaderSummary};
use crate::domain::DeviceLocalDateTime;
use opap_channels::{ResmedFileKind, resmed_signal_prefix};
use opap_edf::{EdfHeader, Limits, ParseError, ParseErrorKind, Parser};
use serde::{Deserialize, Serialize};
use std::{error, fmt};

const MAX_SIGNALS: usize = 256;
const MAX_RECORDS: usize = 65_536;
const MAX_SIGNAL_RECORDS: usize = 262_144;
const MAX_ANNOTATION_BYTES: usize = 8 * 1024 * 1024;
const MAX_ANNOTATIONS: usize = 262_144;
const MAX_ANNOTATION_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_EVENT_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;
const MAX_HEADER_DRIFT_MS: u64 = 6 * 60 * 60 * 1_000;

/// Largest complete, uncompressed ResMed EVE EDF accepted by this decoder.
pub const RESMED_EVE_MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

const EVE_LIMITS: Limits = Limits {
    max_signals: MAX_SIGNALS,
    max_records: MAX_RECORDS,
    max_signal_records: MAX_SIGNAL_RECORDS,
    // Annotation storage is represented as 16-bit EDF samples.
    max_total_samples: MAX_ANNOTATION_BYTES / 2,
    max_annotation_bytes: MAX_ANNOTATION_BYTES,
    max_annotation_records: MAX_SIGNAL_RECORDS,
    max_annotations: MAX_ANNOTATIONS,
    max_annotation_text_bytes: MAX_ANNOTATION_TEXT_BYTES,
};

/// Caller-supplied identity policy for one EVE decode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EveDecodeOptions<'a> {
    /// Identification serial expected in the EDF `SRN=` recording token.
    ///
    /// Production imports should always supply this value. `None` is an
    /// explicit unverified mode intended for internal decoding and fixtures.
    pub expected_serial: Option<&'a str>,
    /// Header summary captured before the complete read, when available.
    ///
    /// Supplying this closes the header portion of an inventory/read TOCTOU
    /// boundary. `None` is explicit unverified mode for annotation-only files
    /// that were not opened during candidate indexing.
    pub expected_header: Option<&'a ResmedEdfHeaderSummary>,
    /// Authoritative filename/index start used to reproduce OSCAR's EVE clock
    /// repair.
    ///
    /// Production import should provide this value. The decoder uses the EDF
    /// header only when its repaired ResMed year is in `2005..=2099` and it is
    /// within six hours (inclusive) of this start; otherwise this authoritative
    /// value becomes the annotation anchor. `None` is explicit unverified mode
    /// for fixtures and internal parser tests.
    pub authoritative_start: Option<&'a ResmedDeviceLocalTime>,
}

/// Whether the decoder verified one caller-provided input property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EveVerification {
    /// The supplied expectation exactly matched the complete source.
    Verified,
    /// The caller explicitly chose not to check this input property.
    NotRequested,
}

/// Clock source selected after applying ResMed's EVE start-time repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EveTimestampAnchor {
    /// A caller-supplied authoritative start was present and the EDF header was
    /// plausible and within OSCAR's six-hour tolerance.
    VerifiedHeader,
    /// The EDF header was implausible or drifted, so the authoritative
    /// filename/index start was used.
    AuthoritativeRepair,
    /// No authoritative start was supplied; the header was used explicitly
    /// without the OSCAR repair check.
    UnverifiedHeader,
}

/// One event classification accepted from a ResMed EVE annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EveEventKind {
    /// Obstructive apnea (`CPAP_Obstructive` in pinned OSCAR).
    ObstructiveApnea,
    /// Hypopnea (`CPAP_Hypopnea` in pinned OSCAR).
    Hypopnea,
    /// Unclassified apnea (`CPAP_Apnea` in pinned OSCAR).
    UnclassifiedApnea,
    /// Respiratory-effort-related arousal (`CPAP_RERA` in pinned OSCAR).
    Rera,
    /// Clear-airway/central apnea (`CPAP_ClearAirway` in pinned OSCAR).
    ClearAirway,
}

impl EveEventKind {
    /// Stable OPAP channel key corresponding to this EVE classification.
    #[must_use]
    pub const fn channel_key(self) -> &'static str {
        match self {
            Self::ObstructiveApnea => "pap.event.obstructive_apnea",
            Self::Hypopnea => "pap.event.hypopnea",
            Self::UnclassifiedApnea => "pap.event.unclassified_apnea",
            Self::Rera => "pap.event.rera",
            Self::ClearAirway => "pap.event.clear_airway",
        }
    }
}

/// One bounded, classified EVE annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveEvent {
    /// Canonical event classification.
    pub kind: EveEventKind,
    /// Zero-based EDF data-record position.
    pub source_record_index: u32,
    /// Zero-based signal position within the data record.
    pub source_signal_index: u16,
    /// Zero-based annotation position within that signal record.
    pub source_annotation_index: u32,
    /// Annotation onset relative to the EDF recording start.
    ///
    /// Pinned OSCAR converts seconds to milliseconds by truncating toward zero;
    /// OPAP retains that behavior at this boundary.
    pub onset_offset_ms: i64,
    /// Device-local wall-clock timestamp calculated from the selected
    /// recording anchor and the annotation onset. It has no implied UTC
    /// offset.
    pub start_time: DeviceLocalDateTime,
    /// Source duration converted to milliseconds by truncating toward zero.
    ///
    /// `None` means no duration was present in the annotation. `Some(0)` is
    /// retained when the source explicitly reported zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Fixed-size, privacy-safe annotation counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveDiagnostics {
    /// Total non-empty annotation texts examined.
    pub source_annotations: u32,
    /// Exact `Recording starts` or `SpO2 Desaturation` records ignored by the
    /// pinned loader.
    pub ignored_annotations: u32,
    /// Non-empty labels that did not map to a supported EVE event.
    ///
    /// The raw text is intentionally not retained.
    pub unknown_annotations: u32,
}

/// Complete bounded result for one uncompressed ResMed EVE EDF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EveEventIndex {
    /// Device-local annotation anchor after optional OSCAR-compatible repair.
    pub recording_start: DeviceLocalDateTime,
    /// Source selected for [`Self::recording_start`].
    pub timestamp_anchor: EveTimestampAnchor,
    /// Result of the caller-requested recording identity check.
    pub serial_verification: EveVerification,
    /// Result of the caller-requested indexed-header consistency check.
    pub header_verification: EveVerification,
    /// Classified annotations in deterministic record-major, signal-major,
    /// source-annotation order.
    pub events: Vec<EveEvent>,
    /// Privacy-safe counts for ignored and unsupported annotations.
    pub diagnostics: EveDiagnostics,
}

/// Privacy-safe category for an EDF parser failure.
///
/// Unlike [`ParseErrorKind`], this type never stores malformed source-field
/// text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EveParseErrorKind {
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
    InvalidFirstRecordTimekeepingOnset,
    NonContiguousRecordTimekeepingOnset,
    MalformedAnnotation,
    Other,
}

impl EveParseErrorKind {
    const fn label(self) -> &'static str {
        match self {
            Self::UnexpectedEof => "unexpected end of input",
            Self::InvalidAscii => "invalid ASCII field",
            Self::InvalidNumber => "invalid numeric field",
            Self::ValueOutOfRange => "numeric field outside its supported range",
            Self::InvalidDateTime => "invalid EDF start date/time",
            Self::HeaderLengthMismatch => "inconsistent header length",
            Self::DataLengthMismatch => "inconsistent record data length",
            Self::UnknownRecordCountWithEmptyRecord => {
                "unknown record count with an empty record layout"
            }
            Self::ZeroByteRecords => "declared records have an empty layout",
            Self::ArithmeticOverflow => "EDF size arithmetic overflow",
            Self::LimitExceeded { .. } => "EDF resource limit exceeded",
            Self::AllocationFailed { .. } => "EDF parser allocation failed",
            Self::MissingTimekeepingSignal => "missing EDF+ timekeeping signal",
            Self::MissingRecordTimekeepingOnset => "missing EDF+ record onset",
            Self::InvalidFirstRecordTimekeepingOnset => "invalid first EDF+ record onset",
            Self::NonContiguousRecordTimekeepingOnset => "non-contiguous EDF+ record onset",
            Self::MalformedAnnotation => "malformed EDF+ annotation",
            Self::Other => "unsupported EDF parse failure",
        }
    }
}

/// Sanitized EDF parser location and category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EveParseError {
    /// Byte offset at which parsing failed.
    pub offset: usize,
    /// Signal index, when applicable.
    pub signal_index: Option<usize>,
    /// Record index, when applicable.
    pub record_index: Option<usize>,
    /// Privacy-safe failure category.
    pub kind: EveParseErrorKind,
}

impl fmt::Display for EveParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "EDF parse failure at byte {}: {}",
            self.offset,
            self.kind.label()
        )?;
        if let EveParseErrorKind::LimitExceeded {
            resource,
            limit,
            actual,
        } = self.kind
        {
            write!(
                formatter,
                " ({resource}; limit {limit}, requested {actual})"
            )?;
        }
        if let EveParseErrorKind::AllocationFailed {
            resource,
            requested,
        } = self.kind
        {
            write!(formatter, " ({resource}; requested {requested})")?;
        }
        if let Some(signal_index) = self.signal_index {
            write!(formatter, ", signal {signal_index}")?;
        }
        if let Some(record_index) = self.record_index {
            write!(formatter, ", record {record_index}")?;
        }
        Ok(())
    }
}

impl error::Error for EveParseError {}

impl From<ParseError> for EveParseError {
    fn from(source: ParseError) -> Self {
        let ParseError {
            offset,
            signal_index,
            record_index,
            kind,
        } = source;
        let kind = match kind {
            ParseErrorKind::UnexpectedEof { .. } => EveParseErrorKind::UnexpectedEof,
            ParseErrorKind::InvalidAscii { .. } => EveParseErrorKind::InvalidAscii,
            ParseErrorKind::InvalidNumber { .. } => EveParseErrorKind::InvalidNumber,
            ParseErrorKind::ValueOutOfRange { .. } => EveParseErrorKind::ValueOutOfRange,
            ParseErrorKind::InvalidDateTime { .. } => EveParseErrorKind::InvalidDateTime,
            ParseErrorKind::HeaderLengthMismatch { .. } => EveParseErrorKind::HeaderLengthMismatch,
            ParseErrorKind::DataLengthMismatch { .. } => EveParseErrorKind::DataLengthMismatch,
            ParseErrorKind::UnknownRecordCountWithEmptyRecord => {
                EveParseErrorKind::UnknownRecordCountWithEmptyRecord
            }
            ParseErrorKind::ZeroByteRecords { .. } => EveParseErrorKind::ZeroByteRecords,
            ParseErrorKind::ArithmeticOverflow { .. } => EveParseErrorKind::ArithmeticOverflow,
            ParseErrorKind::LimitExceeded {
                resource,
                limit,
                actual,
            } => EveParseErrorKind::LimitExceeded {
                resource,
                limit,
                actual,
            },
            ParseErrorKind::AllocationFailed {
                resource,
                requested,
            } => EveParseErrorKind::AllocationFailed {
                resource,
                requested,
            },
            ParseErrorKind::MissingTimekeepingSignal => EveParseErrorKind::MissingTimekeepingSignal,
            ParseErrorKind::MissingRecordTimekeepingOnset => {
                EveParseErrorKind::MissingRecordTimekeepingOnset
            }
            ParseErrorKind::InvalidFirstRecordTimekeepingOnset => {
                EveParseErrorKind::InvalidFirstRecordTimekeepingOnset
            }
            ParseErrorKind::NonContiguousRecordTimekeepingOnset => {
                EveParseErrorKind::NonContiguousRecordTimekeepingOnset
            }
            ParseErrorKind::MalformedAnnotation { .. } => EveParseErrorKind::MalformedAnnotation,
            _ => EveParseErrorKind::Other,
        };
        Self {
            offset,
            signal_index,
            record_index,
            kind,
        }
    }
}

/// Failure to decode a trustworthy EVE annotation source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EveDecodeError {
    /// The complete input exceeded the decoder's byte ceiling.
    FileTooLarge { limit: usize, actual: usize },
    /// An explicitly requested serial check had an empty expected value.
    EmptyExpectedSerial,
    /// The bounded EDF parser rejected the input. Attacker-controlled field
    /// text is removed before this value is retained.
    Parse(EveParseError),
    /// EVE must contain at least one annotation signal.
    MissingAnnotationSignal,
    /// EVE is annotation-only, but the header declared a sampled signal.
    NonAnnotationSignal { signal_index: usize },
    /// An annotation signal declared an empty record layout.
    EmptyAnnotationRecord { signal_index: usize },
    /// Bytes remained after all declared EDF records.
    TrailingData { bytes: usize },
    /// The caller requested identity verification but no non-empty `SRN=`
    /// token was present.
    MissingSerial,
    /// The recording `SRN=` token did not match the selected card.
    SerialMismatch,
    /// More than one non-empty `SRN=` token made the recording identity
    /// ambiguous.
    AmbiguousSerial,
    /// The complete header did not match the caller's indexed summary.
    HeaderMismatch,
    /// A supplied authoritative filename/index start was non-canonical or
    /// outside the supported device-local calendar.
    InvalidAuthoritativeStart,
    /// An annotation onset was outside the supported finite seven-day window.
    OnsetOutOfRange {
        record_index: usize,
        signal_index: usize,
        annotation_index: usize,
    },
    /// A present duration was negative, non-finite, or longer than seven days.
    DurationOutOfRange {
        record_index: usize,
        signal_index: usize,
        annotation_index: usize,
    },
    /// Header-start plus annotation offset exceeded the supported calendar.
    DateRange {
        record_index: usize,
        signal_index: usize,
        annotation_index: usize,
    },
    /// A bounded output allocation could not be satisfied.
    AllocationFailed { requested: usize },
}

impl fmt::Display for EveDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { limit, actual } => write!(
                formatter,
                "EVE EDF exceeds the {limit}-byte input limit ({actual} bytes)"
            ),
            Self::EmptyExpectedSerial => {
                formatter.write_str("expected ResMed serial must not be empty")
            }
            Self::Parse(source) => write!(formatter, "could not parse bounded EVE EDF: {source}"),
            Self::MissingAnnotationSignal => {
                formatter.write_str("EVE EDF has no annotation signal")
            }
            Self::NonAnnotationSignal { signal_index } => write!(
                formatter,
                "EVE EDF signal {signal_index} is sampled data, not annotations"
            ),
            Self::EmptyAnnotationRecord { signal_index } => write!(
                formatter,
                "EVE EDF annotation signal {signal_index} has an empty record layout"
            ),
            Self::TrailingData { bytes } => write!(
                formatter,
                "EVE EDF has {bytes} trailing bytes after its declared records"
            ),
            Self::MissingSerial => {
                formatter.write_str("EVE recording identity is missing its SRN token")
            }
            Self::SerialMismatch => formatter
                .write_str("EVE recording identity does not match the selected ResMed card"),
            Self::AmbiguousSerial => {
                formatter.write_str("EVE recording identity has multiple SRN tokens")
            }
            Self::HeaderMismatch => {
                formatter.write_str("complete EVE header does not match its indexed summary")
            }
            Self::InvalidAuthoritativeStart => formatter.write_str(
                "authoritative EVE filename/index start is not a canonical device-local time",
            ),
            Self::OnsetOutOfRange {
                record_index,
                signal_index,
                annotation_index,
            } => write!(
                formatter,
                "EVE annotation {annotation_index} in record {record_index}, signal {signal_index} has an unsupported onset"
            ),
            Self::DurationOutOfRange {
                record_index,
                signal_index,
                annotation_index,
            } => write!(
                formatter,
                "EVE annotation {annotation_index} in record {record_index}, signal {signal_index} has an unsupported duration"
            ),
            Self::DateRange {
                record_index,
                signal_index,
                annotation_index,
            } => write!(
                formatter,
                "EVE annotation {annotation_index} in record {record_index}, signal {signal_index} exceeds the supported calendar"
            ),
            Self::AllocationFailed { requested } => write!(
                formatter,
                "could not reserve output for {requested} EVE annotations"
            ),
        }
    }
}

impl error::Error for EveDecodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Parse(source) => Some(source),
            _ => None,
        }
    }
}

impl From<ParseError> for EveDecodeError {
    fn from(source: ParseError) -> Self {
        Self::Parse(source.into())
    }
}

/// Decode classified events from one complete, uncompressed ResMed EVE EDF.
///
/// Matching follows pinned OSCAR's case-insensitive
/// label-starts-with-known-alias rule. Unknown labels are counted without
/// retaining their potentially sensitive text. Exact `Recording starts` and
/// `SpO2 Desaturation` annotations are intentionally ignored.
///
/// # Errors
///
/// Returns [`EveDecodeError`] for an over-limit, malformed, structurally
/// incompatible, wrongly identified, or out-of-range input.
pub fn decode_eve(
    bytes: &[u8],
    options: EveDecodeOptions<'_>,
) -> Result<EveEventIndex, EveDecodeError> {
    decode_eve_with_limits(bytes, options, EVE_LIMITS)
}

fn decode_eve_with_limits(
    bytes: &[u8],
    options: EveDecodeOptions<'_>,
    limits: Limits,
) -> Result<EveEventIndex, EveDecodeError> {
    if bytes.len() > RESMED_EVE_MAX_FILE_BYTES {
        return Err(EveDecodeError::FileTooLarge {
            limit: RESMED_EVE_MAX_FILE_BYTES,
            actual: bytes.len(),
        });
    }
    if options.expected_serial.is_some_and(str::is_empty) {
        return Err(EveDecodeError::EmptyExpectedSerial);
    }

    let parser = Parser::new(limits);
    let header = parser.parse_header(bytes)?;
    validate_header_shape(&header.signals)?;
    let header_recording_start = header_start(header.start);
    let header_verification =
        verify_header(&header, &header_recording_start, options.expected_header)?;
    let (recording_start, timestamp_anchor) =
        select_recording_start(header_recording_start, options.authoritative_start)?;
    let recording_start_ms = local_millis(&recording_start).ok_or(EveDecodeError::DateRange {
        record_index: 0,
        signal_index: 0,
        annotation_index: 0,
    })?;
    let serial_verification = verify_serial(&header.recording_id, options.expected_serial)?;

    let parsed = parser.parse(bytes)?;
    if parsed.trailing_data_bytes() != 0 {
        return Err(EveDecodeError::TrailingData {
            bytes: parsed.trailing_data_bytes(),
        });
    }

    let annotation_count = parsed
        .signals()
        .iter()
        .filter_map(|signal| signal.annotation_records())
        .try_fold(0usize, |total, records| {
            records.iter().try_fold(total, |subtotal, record| {
                subtotal.checked_add(record.annotations.len())
            })
        })
        .ok_or(EveDecodeError::AllocationFailed {
            requested: MAX_ANNOTATIONS,
        })?;
    let mut events = Vec::new();
    events
        .try_reserve_exact(annotation_count)
        .map_err(|_| EveDecodeError::AllocationFailed {
            requested: annotation_count,
        })?;

    let mut diagnostics = EveDiagnostics::default();
    for record in parsed.records() {
        for signal_index in 0..parsed.signals().len() {
            let annotations = record
                .annotations(signal_index)
                .ok_or(EveDecodeError::NonAnnotationSignal { signal_index })?;
            for (annotation_index, annotation) in annotations.iter().enumerate() {
                diagnostics.source_annotations = diagnostics
                    .source_annotations
                    .checked_add(1)
                    .ok_or(EveDecodeError::AllocationFailed {
                        requested: annotation_count,
                    })?;

                if matches!(
                    annotation.text.as_str(),
                    "Recording starts" | "SpO2 Desaturation"
                ) {
                    diagnostics.ignored_annotations += 1;
                    continue;
                }

                let Some(kind) = classify_event(&annotation.text) else {
                    diagnostics.unknown_annotations += 1;
                    continue;
                };
                let location = AnnotationLocation {
                    record_index: record.index(),
                    signal_index,
                    annotation_index,
                };
                let onset_offset_ms = onset_millis(annotation.onset_seconds, location)?;
                let start_ms = recording_start_ms
                    .checked_add(onset_offset_ms)
                    .ok_or_else(|| location.date_range())?;
                let start_time =
                    local_datetime_from_millis(start_ms).ok_or_else(|| location.date_range())?;
                let duration_ms = annotation
                    .duration_seconds
                    .map(|duration| duration_millis(duration, location))
                    .transpose()?;

                events.push(EveEvent {
                    kind,
                    source_record_index: u32::try_from(record.index())
                        .expect("EVE record limit fits u32"),
                    source_signal_index: u16::try_from(signal_index)
                        .expect("EVE signal limit fits u16"),
                    source_annotation_index: u32::try_from(annotation_index)
                        .expect("EVE annotation limit fits u32"),
                    onset_offset_ms,
                    start_time,
                    duration_ms,
                });
            }
        }
    }

    Ok(EveEventIndex {
        recording_start,
        timestamp_anchor,
        serial_verification,
        header_verification,
        events,
        diagnostics,
    })
}

fn validate_header_shape(signals: &[opap_edf::SignalHeader]) -> Result<(), EveDecodeError> {
    if signals.is_empty() {
        return Err(EveDecodeError::MissingAnnotationSignal);
    }
    for (signal_index, signal) in signals.iter().enumerate() {
        if !signal.is_annotation_signal() {
            return Err(EveDecodeError::NonAnnotationSignal { signal_index });
        }
        if signal.samples_per_record == 0 {
            return Err(EveDecodeError::EmptyAnnotationRecord { signal_index });
        }
    }
    Ok(())
}

fn verify_serial(
    recording_id: &str,
    expected_serial: Option<&str>,
) -> Result<EveVerification, EveDecodeError> {
    let mut serials = recording_serials(recording_id);
    let actual = serials.next();
    if serials.next().is_some() {
        return Err(EveDecodeError::AmbiguousSerial);
    }
    let Some(expected) = expected_serial else {
        return Ok(EveVerification::NotRequested);
    };
    match actual {
        Some(actual) if actual == expected => Ok(EveVerification::Verified),
        Some(_) => Err(EveDecodeError::SerialMismatch),
        None => Err(EveDecodeError::MissingSerial),
    }
}

fn recording_serials(recording_id: &str) -> impl Iterator<Item = &str> {
    recording_id
        .split_ascii_whitespace()
        .filter_map(|token| token.strip_prefix("SRN="))
        .filter(|serial| !serial.is_empty())
}

fn verify_header(
    header: &EdfHeader,
    recording_start: &DeviceLocalDateTime,
    expected: Option<&ResmedEdfHeaderSummary>,
) -> Result<EveVerification, EveDecodeError> {
    let Some(expected) = expected else {
        return Ok(EveVerification::NotRequested);
    };
    let structural_match = u64::try_from(header.header_bytes).ok() == Some(expected.header_bytes)
        && u16::try_from(header.signals.len()).ok() == Some(expected.signal_count)
        && header
            .declared_record_count
            .and_then(|count| u64::try_from(count).ok())
            == expected.declared_record_count
        && header.record_duration_seconds.to_bits() == expected.record_duration_seconds.to_bits()
        && estimated_duration_millis(header) == expected.estimated_duration_millis;
    let start_match = expected
        .start_time
        .as_ref()
        .is_some_and(|start| summary_start_matches(start, recording_start));
    if structural_match && start_match {
        Ok(EveVerification::Verified)
    } else {
        Err(EveDecodeError::HeaderMismatch)
    }
}

fn estimated_duration_millis(header: &EdfHeader) -> Option<u64> {
    let records = u32::try_from(header.declared_record_count?).ok()?;
    let seconds = header.record_duration_seconds * f64::from(records);
    if !seconds.is_finite() || !(0.0..=MAX_EVENT_SECONDS).contains(&seconds) {
        return None;
    }
    // Pinned OSCAR truncates the floating end calculation to whole seconds
    // during indexing. Keep verification on that exact derived boundary.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole_seconds = seconds as u64;
    whole_seconds.checked_mul(1_000)
}

fn summary_start_matches(expected: &ResmedDeviceLocalTime, actual: &DeviceLocalDateTime) -> bool {
    indexed_start(expected).as_ref() == Some(actual)
}

fn select_recording_start(
    header_start: DeviceLocalDateTime,
    authoritative: Option<&ResmedDeviceLocalTime>,
) -> Result<(DeviceLocalDateTime, EveTimestampAnchor), EveDecodeError> {
    let Some(authoritative) = authoritative else {
        return Ok((header_start, EveTimestampAnchor::UnverifiedHeader));
    };
    let authoritative =
        indexed_start(authoritative).ok_or(EveDecodeError::InvalidAuthoritativeStart)?;
    let header_millis =
        local_millis(&header_start).ok_or(EveDecodeError::InvalidAuthoritativeStart)?;
    let authoritative_millis =
        local_millis(&authoritative).ok_or(EveDecodeError::InvalidAuthoritativeStart)?;
    let header_is_plausible = (2_005..=2_099).contains(&header_start.year)
        && header_millis.abs_diff(authoritative_millis) <= MAX_HEADER_DRIFT_MS;
    if header_is_plausible {
        Ok((header_start, EveTimestampAnchor::VerifiedHeader))
    } else {
        Ok((authoritative, EveTimestampAnchor::AuthoritativeRepair))
    }
}

fn indexed_start(value: &ResmedDeviceLocalTime) -> Option<DeviceLocalDateTime> {
    let canonical_wall_time = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        value.year, value.month, value.day, value.hour, value.minute, value.second
    );
    if value.wall_time != canonical_wall_time || value.millisecond != 0 {
        return None;
    }
    let candidate = DeviceLocalDateTime {
        year: value.year,
        month: value.month,
        day: value.day,
        hour: value.hour,
        minute: value.minute,
        second: value.second,
        millisecond: value.millisecond,
    };
    valid_local_datetime(&candidate).then_some(candidate)
}

fn classify_event(text: &str) -> Option<EveEventKind> {
    let channel = resmed_signal_prefix(ResmedFileKind::Eve, text)?;
    match channel.key.as_str() {
        "pap.event.obstructive_apnea" => Some(EveEventKind::ObstructiveApnea),
        "pap.event.hypopnea" => Some(EveEventKind::Hypopnea),
        "pap.event.unclassified_apnea" => Some(EveEventKind::UnclassifiedApnea),
        "pap.event.rera" => Some(EveEventKind::Rera),
        "pap.event.clear_airway" => Some(EveEventKind::ClearAirway),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct AnnotationLocation {
    record_index: usize,
    signal_index: usize,
    annotation_index: usize,
}

impl AnnotationLocation {
    const fn onset_out_of_range(self) -> EveDecodeError {
        EveDecodeError::OnsetOutOfRange {
            record_index: self.record_index,
            signal_index: self.signal_index,
            annotation_index: self.annotation_index,
        }
    }

    const fn duration_out_of_range(self) -> EveDecodeError {
        EveDecodeError::DurationOutOfRange {
            record_index: self.record_index,
            signal_index: self.signal_index,
            annotation_index: self.annotation_index,
        }
    }

    const fn date_range(self) -> EveDecodeError {
        EveDecodeError::DateRange {
            record_index: self.record_index,
            signal_index: self.signal_index,
            annotation_index: self.annotation_index,
        }
    }
}

fn onset_millis(seconds: f64, location: AnnotationLocation) -> Result<i64, EveDecodeError> {
    if !seconds.is_finite() || seconds.abs() > MAX_EVENT_SECONDS {
        return Err(location.onset_out_of_range());
    }
    let millis = (seconds * 1_000.0).trunc();
    if millis < i64::MIN as f64 || millis > i64::MAX as f64 {
        return Err(location.onset_out_of_range());
    }
    Ok(millis as i64)
}

fn duration_millis(seconds: f64, location: AnnotationLocation) -> Result<u64, EveDecodeError> {
    if !seconds.is_finite() || !(0.0..=MAX_EVENT_SECONDS).contains(&seconds) {
        return Err(location.duration_out_of_range());
    }
    let millis = (seconds * 1_000.0).trunc();
    if millis > u64::MAX as f64 {
        return Err(location.duration_out_of_range());
    }
    Ok(millis as u64)
}

fn header_start(start: opap_edf::EdfDateTime) -> DeviceLocalDateTime {
    DeviceLocalDateTime {
        // Pinned ResMed handling adds 100 years to EDF's conventional
        // 1985..1999 pivot result, so every two-digit device year denotes
        // 2000..2099.
        year: 2_000 + u16::from(start.year_two_digits),
        month: start.month,
        day: start.day,
        hour: start.hour,
        minute: start.minute,
        second: start.second,
        millisecond: 0,
    }
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
    let hour = u8::try_from(within_day / 3_600_000).ok()?;
    let minute = u8::try_from((within_day % 3_600_000) / 60_000).ok()?;
    let second = u8::try_from((within_day % 60_000) / 1_000).ok()?;
    let millisecond = u16::try_from(within_day % 1_000).ok()?;
    Some(DeviceLocalDateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        millisecond,
    })
}

// Howard Hinnant's civil-calendar conversion, adjusted to the Unix epoch.
fn days_from_civil(year: u16, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - if month <= 2 { 1 } else { 0 };
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
    year += if month <= 2 { 1 } else { 0 };
    Some((
        u16::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK_BYTES: usize = 256;

    fn field(value: &str, width: usize) -> Vec<u8> {
        assert!(value.len() <= width);
        let mut bytes = vec![b' '; width];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        bytes
    }

    fn annotation_block(tals: &[&[u8]]) -> Vec<u8> {
        let mut block = Vec::new();
        for tal in tals {
            block.extend_from_slice(tal);
        }
        assert!(block.len() <= BLOCK_BYTES);
        block.resize(BLOCK_BYTES, 0);
        block
    }

    fn synthetic_eve(
        recording_id: &str,
        reserved: &str,
        labels: &[&str],
        records: &[Vec<Vec<u8>>],
    ) -> Vec<u8> {
        synthetic_eve_with_layout(
            recording_id,
            reserved,
            labels,
            &vec![BLOCK_BYTES / 2; labels.len()],
            &records.len().to_string(),
            records,
        )
    }

    fn synthetic_eve_with_layout(
        recording_id: &str,
        reserved: &str,
        labels: &[&str],
        samples_per_record: &[usize],
        record_count: &str,
        records: &[Vec<Vec<u8>>],
    ) -> Vec<u8> {
        assert_eq!(labels.len(), samples_per_record.len());
        let header_bytes = 256 + labels.len() * 256;
        let mut bytes = Vec::new();
        bytes.extend(field("0", 8));
        bytes.extend(field("patient", 80));
        bytes.extend(field(recording_id, 80));
        bytes.extend_from_slice(b"29.02.2423.59.59");
        bytes.extend(field(&header_bytes.to_string(), 8));
        bytes.extend(field(reserved, 44));
        bytes.extend(field(record_count, 8));
        bytes.extend(field("1", 8));
        bytes.extend(field(&labels.len().to_string(), 4));

        for label in labels {
            bytes.extend(field(label, 16));
        }
        for _ in labels {
            bytes.extend(field("", 80));
        }
        for _ in labels {
            bytes.extend(field("", 8));
        }
        for _ in labels {
            bytes.extend(field("-1", 8));
        }
        for _ in labels {
            bytes.extend(field("1", 8));
        }
        for _ in labels {
            bytes.extend(field("-32768", 8));
        }
        for _ in labels {
            bytes.extend(field("32767", 8));
        }
        for _ in labels {
            bytes.extend(field("", 80));
        }
        for samples in samples_per_record {
            bytes.extend(field(&samples.to_string(), 8));
        }
        for _ in labels {
            bytes.extend(field("", 32));
        }
        assert_eq!(bytes.len(), header_bytes);

        for record in records {
            assert_eq!(record.len(), labels.len());
            for (signal_index, block) in record.iter().enumerate() {
                assert_eq!(block.len(), samples_per_record[signal_index] * 2);
                bytes.extend_from_slice(block);
            }
        }
        bytes
    }

    fn decode(bytes: &[u8]) -> Result<EveEventIndex, EveDecodeError> {
        decode_eve(
            bytes,
            EveDecodeOptions {
                expected_serial: Some("serial-123"),
                expected_header: None,
                authoritative_start: None,
            },
        )
    }

    fn header_summary(bytes: &[u8]) -> ResmedEdfHeaderSummary {
        let header = Parser::default()
            .parse_header(bytes)
            .expect("fixture header");
        let start = header_start(header.start);
        ResmedEdfHeaderSummary {
            start_time: Some(ResmedDeviceLocalTime {
                wall_time: format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                    start.year, start.month, start.day, start.hour, start.minute, start.second
                ),
                year: start.year,
                month: start.month,
                day: start.day,
                hour: start.hour,
                minute: start.minute,
                second: start.second,
                millisecond: start.millisecond,
            }),
            header_bytes: u64::try_from(header.header_bytes).expect("header bytes fit"),
            signal_count: u16::try_from(header.signals.len()).expect("signal count fits"),
            declared_record_count: header
                .declared_record_count
                .map(|count| u64::try_from(count).expect("record count fits")),
            record_duration_seconds: header.record_duration_seconds,
            estimated_duration_millis: estimated_duration_millis(&header),
        }
    }

    fn indexed_time(
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> ResmedDeviceLocalTime {
        ResmedDeviceLocalTime {
            wall_time: format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"),
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond: 0,
        }
    }

    #[test]
    fn maps_all_pinned_event_prefixes_case_insensitively() {
        let block = annotation_block(&[
            b"+0\x14ObStRuCtIvE ApNeA subtype\x14\0",
            b"+1\x14HYPOPNEA-long\x14\0",
            b"+2\x14apnea subtype\x14\0",
            b"+3\x14AROUSAL marker\x14\0",
            b"+4\x14central APNEA marker\x14\0",
        ]);
        let bytes = synthetic_eve(
            "ResMed SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[vec![block]],
        );
        let decoded = decode(&bytes).expect("all five pinned aliases");

        assert_eq!(
            decoded
                .events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            vec![
                EveEventKind::ObstructiveApnea,
                EveEventKind::Hypopnea,
                EveEventKind::UnclassifiedApnea,
                EveEventKind::Rera,
                EveEventKind::ClearAirway,
            ]
        );
        assert_eq!(
            decoded
                .events
                .iter()
                .map(|event| event.kind.channel_key())
                .collect::<Vec<_>>(),
            vec![
                "pap.event.obstructive_apnea",
                "pap.event.hypopnea",
                "pap.event.unclassified_apnea",
                "pap.event.rera",
                "pap.event.clear_airway",
            ]
        );
        assert_eq!(decoded.serial_verification, EveVerification::Verified);
        assert_eq!(decoded.header_verification, EveVerification::NotRequested);
        assert_eq!(
            decoded.timestamp_anchor,
            EveTimestampAnchor::UnverifiedHeader
        );
        assert_eq!(
            decoded.diagnostics,
            EveDiagnostics {
                source_annotations: 5,
                ignored_annotations: 0,
                unknown_annotations: 0,
            }
        );
    }

    #[test]
    fn exact_ignored_labels_do_not_suppress_prefixes_or_other_casing() {
        let block = annotation_block(&[
            b"+0\x14Recording starts\x14\0",
            b"+1\x14SpO2 Desaturation\x14\0",
            b"+2\x14Recording starts extra\x14\0",
            b"+3\x14SpO2 Desaturation extra\x14\0",
            b"+4\x14recording starts\x14\0",
            b"+5\x14unobserved private annotation\x14\0",
        ]);
        let bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);
        let decoded = decode(&bytes).expect("unknown annotations are diagnostics");

        assert!(decoded.events.is_empty());
        assert_eq!(
            decoded.diagnostics,
            EveDiagnostics {
                source_annotations: 6,
                ignored_annotations: 2,
                unknown_annotations: 4,
            }
        );
    }

    #[test]
    fn preserves_missing_and_explicit_zero_durations_and_crosses_midnight() {
        let block = annotation_block(&[
            b"+0.125\x14Hypopnea\x14\0",
            b"+1.5\x150\x14Apnea\x14\0",
            b"+2.9999\x151.2349\x14Central apnea\x14\0",
        ]);
        let bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);
        let decoded = decode(&bytes).expect("valid duration variants");

        assert_eq!(decoded.events[0].onset_offset_ms, 125);
        assert_eq!(decoded.events[0].duration_ms, None);
        assert_eq!(decoded.events[1].onset_offset_ms, 1_500);
        assert_eq!(decoded.events[1].duration_ms, Some(0));
        assert_eq!(decoded.events[2].onset_offset_ms, 2_999);
        assert_eq!(decoded.events[2].duration_ms, Some(1_234));
        assert_eq!(
            decoded.recording_start,
            DeviceLocalDateTime {
                year: 2024,
                month: 2,
                day: 29,
                hour: 23,
                minute: 59,
                second: 59,
                millisecond: 0,
            }
        );
        assert_eq!(
            decoded.events[1].start_time,
            DeviceLocalDateTime {
                year: 2024,
                month: 3,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 500,
            }
        );
    }

    #[test]
    fn applies_resmeds_full_2000_century_header_policy() {
        let block = annotation_block(&[b"+2\x14Hypopnea\x14\0"]);
        let mut bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);
        bytes[168..184].copy_from_slice(b"31.12.9923.59.59");

        let decoded = decode(&bytes).expect("ResMed year 99");
        assert_eq!(
            decoded.timestamp_anchor,
            EveTimestampAnchor::UnverifiedHeader
        );
        assert_eq!(
            decoded.recording_start,
            DeviceLocalDateTime {
                year: 2099,
                month: 12,
                day: 31,
                hour: 23,
                minute: 59,
                second: 59,
                millisecond: 0,
            }
        );
        assert_eq!(
            decoded.events[0].start_time,
            DeviceLocalDateTime {
                year: 2100,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 1,
                millisecond: 0,
            }
        );
    }

    #[test]
    fn applies_oscar_start_repair_with_an_inclusive_six_hour_tolerance() {
        let block = annotation_block(&[b"+2\x14Hypopnea\x14\0"]);
        let bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);

        let exactly_six_hours = indexed_time(2024, 3, 1, 5, 59, 59);
        let accepted = decode_eve(
            &bytes,
            EveDecodeOptions {
                expected_serial: Some("serial-123"),
                expected_header: None,
                authoritative_start: Some(&exactly_six_hours),
            },
        )
        .expect("six-hour boundary uses the plausible EDF header");
        assert_eq!(
            accepted.timestamp_anchor,
            EveTimestampAnchor::VerifiedHeader
        );
        assert_eq!(accepted.recording_start.year, 2024);
        assert_eq!(accepted.recording_start.month, 2);
        assert_eq!(accepted.recording_start.day, 29);
        assert_eq!(accepted.recording_start.hour, 23);
        assert_eq!(accepted.recording_start.minute, 59);
        assert_eq!(accepted.recording_start.second, 59);

        let six_hours_and_one_second = indexed_time(2024, 3, 1, 6, 0, 0);
        let repaired = decode_eve(
            &bytes,
            EveDecodeOptions {
                expected_serial: Some("serial-123"),
                expected_header: None,
                authoritative_start: Some(&six_hours_and_one_second),
            },
        )
        .expect("drift outside the tolerance uses the authoritative start");
        assert_eq!(
            repaired.timestamp_anchor,
            EveTimestampAnchor::AuthoritativeRepair
        );
        assert_eq!(
            repaired.recording_start,
            DeviceLocalDateTime {
                year: 2024,
                month: 3,
                day: 1,
                hour: 6,
                minute: 0,
                second: 0,
                millisecond: 0,
            }
        );
        assert_eq!(
            repaired.events[0].start_time,
            DeviceLocalDateTime {
                year: 2024,
                month: 3,
                day: 1,
                hour: 6,
                minute: 0,
                second: 2,
                millisecond: 0,
            }
        );
    }

    #[test]
    fn repairs_future_century_and_pre_2005_headers_to_authoritative_start() {
        let block = annotation_block(&[b"+2\x14Hypopnea\x14\0"]);
        let mut future = synthetic_eve(
            "SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[vec![block.clone()]],
        );
        future[168..184].copy_from_slice(b"31.12.9923.59.59");
        let authoritative = indexed_time(2026, 1, 1, 12, 0, 0);
        let repaired = decode_eve(
            &future,
            EveDecodeOptions {
                expected_serial: Some("serial-123"),
                expected_header: None,
                authoritative_start: Some(&authoritative),
            },
        )
        .expect("future-century header is repaired");
        assert_eq!(
            repaired.timestamp_anchor,
            EveTimestampAnchor::AuthoritativeRepair
        );
        assert_eq!(repaired.recording_start.year, 2026);
        assert_eq!(repaired.events[0].start_time.year, 2026);
        assert_eq!(repaired.events[0].start_time.second, 2);

        let mut too_early =
            synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);
        too_early[168..184].copy_from_slice(b"01.01.0412.00.00");
        let same_wall_time = indexed_time(2004, 1, 1, 12, 0, 0);
        let repaired = decode_eve(
            &too_early,
            EveDecodeOptions {
                expected_serial: Some("serial-123"),
                expected_header: None,
                authoritative_start: Some(&same_wall_time),
            },
        )
        .expect("pre-2005 header is repaired even without clock drift");
        assert_eq!(
            repaired.timestamp_anchor,
            EveTimestampAnchor::AuthoritativeRepair
        );
        assert_eq!(repaired.recording_start.year, 2004);
    }

    #[test]
    fn rejects_noncanonical_authoritative_start() {
        let block = annotation_block(&[b"+0\x14Hypopnea\x14\0"]);
        let bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);
        let mut authoritative = indexed_time(2024, 2, 29, 23, 59, 59);
        authoritative.wall_time = "private malformed timestamp".to_owned();

        let error = decode_eve(
            &bytes,
            EveDecodeOptions {
                expected_serial: Some("serial-123"),
                expected_header: None,
                authoritative_start: Some(&authoritative),
            },
        )
        .expect_err("authoritative start must be canonical");
        assert_eq!(error, EveDecodeError::InvalidAuthoritativeStart);
        assert!(!format!("{error:?}").contains("private malformed timestamp"));
    }

    #[test]
    fn emits_events_in_record_major_then_signal_major_source_order() {
        let records = [
            vec![
                annotation_block(&[b"+3\x14Hypopnea\x14\0"]),
                annotation_block(&[b"+1\x14Arousal\x14\0"]),
            ],
            vec![
                annotation_block(&[b"+2\x14Central apnea\x14\0"]),
                annotation_block(&[b"+0\x14Obstructive apnea\x14\0"]),
            ],
        ];
        let bytes = synthetic_eve(
            "SRN=serial-123",
            "",
            &["EDF Annotations", "Aux Annotations"],
            &records,
        );
        let decoded = decode(&bytes).expect("multiple records and annotation signals");

        assert_eq!(
            decoded
                .events
                .iter()
                .map(|event| (
                    event.source_record_index,
                    event.source_signal_index,
                    event.source_annotation_index,
                    event.kind,
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, 0, 0, EveEventKind::Hypopnea),
                (0, 1, 0, EveEventKind::Rera),
                (1, 0, 0, EveEventKind::ClearAirway),
                (1, 1, 0, EveEventKind::ObstructiveApnea),
            ]
        );
    }

    #[test]
    fn accepts_valid_edf_plus_timekeeping_without_emitting_it() {
        let first = annotation_block(&[b"+0\x14\x14\0", b"+0.25\x14Hypopnea\x14\0"]);
        let second = annotation_block(&[b"+1\x14\x14\0", b"+1.25\x14Apnea\x14\0"]);
        let bytes = synthetic_eve(
            "SRN=serial-123",
            "EDF+C",
            &["EDF Annotations"],
            &[vec![first], vec![second]],
        );
        let decoded = decode(&bytes).expect("valid EDF+C EVE");

        assert_eq!(decoded.events.len(), 2);
        assert_eq!(decoded.diagnostics.source_annotations, 2);
    }

    #[test]
    fn rejects_malformed_annotations_and_non_annotation_shapes() {
        let mut malformed = b"+0\x14Hypopnea".to_vec();
        malformed.resize(BLOCK_BYTES, b'x');
        let bytes = synthetic_eve(
            "SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[vec![malformed]],
        );
        assert!(matches!(
            decode(&bytes),
            Err(EveDecodeError::Parse(EveParseError {
                kind: EveParseErrorKind::MalformedAnnotation,
                ..
            }))
        ));

        let sampled = synthetic_eve(
            "SRN=serial-123",
            "",
            &["Flow"],
            &[vec![vec![0; BLOCK_BYTES]]],
        );
        assert_eq!(
            decode(&sampled),
            Err(EveDecodeError::NonAnnotationSignal { signal_index: 0 })
        );

        let empty =
            synthetic_eve_with_layout("SRN=serial-123", "", &["EDF Annotations"], &[0], "0", &[]);
        assert_eq!(
            decode(&empty),
            Err(EveDecodeError::EmptyAnnotationRecord { signal_index: 0 })
        );
    }

    #[test]
    fn parse_errors_do_not_retain_private_header_text() {
        let block = annotation_block(&[b"+0\x14Hypopnea\x14\0"]);
        let mut bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);
        bytes[236..244].copy_from_slice(b"SECRET!!");

        let error = decode(&bytes).expect_err("private marker is not a numeric record count");
        assert!(matches!(
            &error,
            EveDecodeError::Parse(EveParseError {
                kind: EveParseErrorKind::InvalidNumber,
                ..
            })
        ));
        assert!(!format!("{error}").contains("SECRET!!"));
        assert!(!format!("{error:?}").contains("SECRET!!"));
    }

    #[test]
    fn enforces_file_record_onset_and_duration_bounds() {
        let too_large = vec![0; RESMED_EVE_MAX_FILE_BYTES + 1];
        assert_eq!(
            decode(&too_large),
            Err(EveDecodeError::FileTooLarge {
                limit: RESMED_EVE_MAX_FILE_BYTES,
                actual: RESMED_EVE_MAX_FILE_BYTES + 1,
            })
        );

        let record_limited = synthetic_eve_with_layout(
            "SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[1],
            &(MAX_RECORDS + 1).to_string(),
            &[],
        );
        assert!(matches!(
            decode(&record_limited),
            Err(EveDecodeError::Parse(EveParseError {
                kind: EveParseErrorKind::LimitExceeded {
                    resource: "records",
                    limit: MAX_RECORDS,
                    actual,
                },
                ..
            })) if actual == MAX_RECORDS + 1
        ));

        let annotation_limited = synthetic_eve(
            "SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[vec![annotation_block(&[
                b"+0\x14Hypopnea\x14\0",
                b"+1\x14Apnea\x14\0",
            ])]],
        );
        let error = decode_eve_with_limits(
            &annotation_limited,
            EveDecodeOptions {
                expected_serial: Some("serial-123"),
                expected_header: None,
                authoritative_start: None,
            },
            Limits {
                max_annotations: 1,
                ..EVE_LIMITS
            },
        )
        .expect_err("annotation object ceiling");
        assert!(matches!(
            error,
            EveDecodeError::Parse(EveParseError {
                kind: EveParseErrorKind::LimitExceeded {
                    resource: "annotations",
                    limit: 1,
                    actual: 2,
                },
                ..
            })
        ));

        let onset = annotation_block(&[b"+604800.001\x14Hypopnea\x14\0"]);
        let bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![onset]]);
        assert_eq!(
            decode(&bytes),
            Err(EveDecodeError::OnsetOutOfRange {
                record_index: 0,
                signal_index: 0,
                annotation_index: 0,
            })
        );

        let negative_duration = annotation_block(&[b"+0\x15-0.001\x14Hypopnea\x14\0".as_slice()]);
        let bytes = synthetic_eve(
            "SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[vec![negative_duration]],
        );
        assert!(matches!(
            decode(&bytes),
            Err(EveDecodeError::Parse(EveParseError {
                kind: EveParseErrorKind::MalformedAnnotation,
                ..
            }))
        ));

        let excessive_duration =
            annotation_block(&[b"+0\x15604800.001\x14Hypopnea\x14\0".as_slice()]);
        let bytes = synthetic_eve(
            "SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[vec![excessive_duration]],
        );
        assert_eq!(
            decode(&bytes),
            Err(EveDecodeError::DurationOutOfRange {
                record_index: 0,
                signal_index: 0,
                annotation_index: 0,
            })
        );
    }

    #[test]
    fn serial_verification_is_strict_optional_and_privacy_safe() {
        let block = annotation_block(&[b"+0\x14Hypopnea\x14\0"]);
        let mismatch = synthetic_eve(
            "ResMed SRN=private-source-serial",
            "",
            &["EDF Annotations"],
            &[vec![block.clone()]],
        );
        let error = decode_eve(
            &mismatch,
            EveDecodeOptions {
                expected_serial: Some("private-expected-serial"),
                expected_header: None,
                authoritative_start: None,
            },
        )
        .expect_err("serial mismatch");
        assert_eq!(error, EveDecodeError::SerialMismatch);
        assert!(!error.to_string().contains("private-source-serial"));
        assert!(!error.to_string().contains("private-expected-serial"));

        let missing = synthetic_eve(
            "ResMed recording",
            "",
            &["EDF Annotations"],
            &[vec![block.clone()]],
        );
        assert_eq!(decode(&missing), Err(EveDecodeError::MissingSerial));

        let unverified = decode_eve(
            &missing,
            EveDecodeOptions {
                expected_serial: None,
                expected_header: None,
                authoritative_start: None,
            },
        )
        .expect("explicit unverified mode");
        assert_eq!(
            unverified.serial_verification,
            EveVerification::NotRequested
        );
        assert_eq!(unverified.events.len(), 1);

        let empty_expected = decode_eve(
            &mismatch,
            EveDecodeOptions {
                expected_serial: Some(""),
                expected_header: None,
                authoritative_start: None,
            },
        )
        .expect_err("empty identity policy");
        assert_eq!(empty_expected, EveDecodeError::EmptyExpectedSerial);
    }

    #[test]
    fn rejects_every_ambiguous_nonempty_serial_token_set() {
        let block = annotation_block(&[b"+0\x14Hypopnea\x14\0"]);
        for recording_id in [
            "SRN=serial-123 SRN=private-other",
            "SRN=serial-123 SRN=serial-123",
        ] {
            let bytes = synthetic_eve(
                recording_id,
                "",
                &["EDF Annotations"],
                &[vec![block.clone()]],
            );
            let error = decode(&bytes).expect_err("multiple non-empty serials are ambiguous");
            assert_eq!(error, EveDecodeError::AmbiguousSerial);
            assert!(!format!("{error}").contains("private-other"));
            assert!(!format!("{error:?}").contains("private-other"));
        }

        let duplicate = synthetic_eve(
            "SRN=serial-123 SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[vec![block.clone()]],
        );
        let error = decode_eve(
            &duplicate,
            EveDecodeOptions {
                expected_serial: None,
                expected_header: None,
                authoritative_start: None,
            },
        )
        .expect_err("unverified mode does not permit ambiguous source identity");
        assert_eq!(error, EveDecodeError::AmbiguousSerial);

        let one_nonempty = synthetic_eve(
            "SRN= SRN=serial-123",
            "",
            &["EDF Annotations"],
            &[vec![block]],
        );
        let decoded = decode(&one_nonempty).expect("empty SRN tokens do not create ambiguity");
        assert_eq!(decoded.serial_verification, EveVerification::Verified);
    }

    #[test]
    fn verifies_an_optional_indexed_header_before_decoding_events() {
        let block = annotation_block(&[b"+0\x14Hypopnea\x14\0"]);
        let bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);
        let summary = header_summary(&bytes);
        let decoded = decode_eve(
            &bytes,
            EveDecodeOptions {
                expected_serial: Some("serial-123"),
                expected_header: Some(&summary),
                authoritative_start: None,
            },
        )
        .expect("complete header matches indexed summary");
        assert_eq!(decoded.header_verification, EveVerification::Verified);

        let mut changed_shape = summary.clone();
        changed_shape.signal_count += 1;
        assert_eq!(
            decode_eve(
                &bytes,
                EveDecodeOptions {
                    expected_serial: Some("serial-123"),
                    expected_header: Some(&changed_shape),
                    authoritative_start: None,
                },
            ),
            Err(EveDecodeError::HeaderMismatch)
        );

        let mut changed_estimate = summary.clone();
        changed_estimate.estimated_duration_millis = Some(9_999);
        assert_eq!(
            decode_eve(
                &bytes,
                EveDecodeOptions {
                    expected_serial: Some("serial-123"),
                    expected_header: Some(&changed_estimate),
                    authoritative_start: None,
                },
            ),
            Err(EveDecodeError::HeaderMismatch)
        );

        let mut missing_start = summary.clone();
        missing_start.start_time = None;
        assert_eq!(
            decode_eve(
                &bytes,
                EveDecodeOptions {
                    expected_serial: Some("serial-123"),
                    expected_header: Some(&missing_start),
                    authoritative_start: None,
                },
            ),
            Err(EveDecodeError::HeaderMismatch)
        );

        let mut changed_start = summary;
        changed_start
            .start_time
            .as_mut()
            .expect("start summary")
            .wall_time = "2099-01-01T00:00:00".to_owned();
        assert_eq!(
            decode_eve(
                &bytes,
                EveDecodeOptions {
                    expected_serial: Some("serial-123"),
                    expected_header: Some(&changed_start),
                    authoritative_start: None,
                },
            ),
            Err(EveDecodeError::HeaderMismatch)
        );
    }

    #[test]
    fn rejects_declared_record_trailing_bytes() {
        let block = annotation_block(&[b"+0\x14Hypopnea\x14\0"]);
        let mut bytes = synthetic_eve("SRN=serial-123", "", &["EDF Annotations"], &[vec![block]]);
        bytes.extend_from_slice(b"trailing-private-data");
        assert_eq!(
            decode(&bytes),
            Err(EveDecodeError::TrailingData {
                bytes: b"trailing-private-data".len(),
            })
        );
    }
}
