// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2025 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from OSCAR:
// https://gitlab.com/CrimsonNape/OSCAR-code
// Upstream commit: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
// Relevant upstream files:
// oscar/SleepLib/loader_plugins/resmed_loader.cpp
// oscar/SleepLib/loader_plugins/resmed_EDFinfo.cpp
// Modified: 2026-07-23

//! Bounded decoding of therapy boundaries from a root-level ResMed `STR.edf`.
//!
//! This module is intentionally independent of session grouping and storage.
//! It decodes only the three signals OSCAR uses to anchor therapy sessions:
//! mask-on minutes, mask-off minutes, and the raw mask-event count. Signal
//! values remain raw EDF digital values; physical calibration is not applied.
//!
//! The pinned OSCAR loader repairs a slot-zero mask-off whose mask-on is absent
//! by starting it at local noon. It also contains an unreachable trailing-mask
//! repair. OPAP implements that intended repair only for a historical record,
//! caps it at the following noon, and omits a still-open boundary on the current
//! or a future device day. These deliberate bounds prevent an unfinished
//! current session from gaining invented usage.

use crate::domain::DeviceLocalDateTime;
use opap_edf::{EdfFile, Limits, ParseError, Parser, Signal};
use serde::{Deserialize, Serialize};
use std::{error, fmt};

const SECONDS_PER_DAY: f64 = 86_400.0;
const MINUTES_PER_DAY: i16 = 24 * 60;
const MAX_SIGNALS: usize = 256;
const MAX_RECORDS: usize = 20_000;
const MAX_SIGNAL_RECORDS: usize = MAX_SIGNALS * MAX_RECORDS;
const MAX_TOTAL_SAMPLES: usize = 16_000_000;
const MAX_MASK_SLOTS_PER_RECORD: usize = 128;

/// Largest uncompressed root `STR.edf` accepted by this decoder.
pub const RESMED_STR_MAX_FILE_BYTES: usize = 32 * 1024 * 1024;

const STR_LIMITS: Limits = Limits {
    max_signals: MAX_SIGNALS,
    max_records: MAX_RECORDS,
    max_signal_records: MAX_SIGNAL_RECORDS,
    max_total_samples: MAX_TOTAL_SAMPLES,
    max_annotation_bytes: 0,
    max_annotation_records: 0,
    max_annotations: 0,
    max_annotation_text_bytes: 0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StrDecodeOptions<'a> {
    expected_serial: Option<&'a str>,
    current_device_local_time: DeviceLocalDateTime,
}

/// Whether the decoder verified the STR recording identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrSerialVerification {
    /// The sole non-empty `SRN=` token exactly matched the expected serial.
    Verified,
    /// The caller explicitly chose not to verify the recording identity.
    NotRequested,
}

/// Which exact ResMed signal-label spelling was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrSignalLabelStyle {
    /// Series 9 spelling containing a space.
    Spaced,
    /// Series 1x spelling without a space.
    Compact,
}

/// Exact labels selected for the three required signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrSelectedSignalLabels {
    pub mask_on: StrSignalLabelStyle,
    pub mask_off: StrSignalLabelStyle,
    pub mask_events: StrSignalLabelStyle,
}

/// A decoded mask-on/mask-off pair within one local-noon STR record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrTherapyBoundary {
    /// Zero-based source slot within the selected mask-on/off signals.
    pub source_slot: u16,
    /// Original signed digital value from the mask-on signal.
    ///
    /// Legacy serialized boundary indexes deserialize this as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_mask_on_value: Option<i16>,
    /// Original signed digital value from the mask-off signal.
    ///
    /// Legacy serialized boundary indexes deserialize this as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_mask_off_value: Option<i16>,
    /// Selected minutes after the record's local-noon start.
    pub mask_on_minute: u16,
    /// Selected minutes after local noon, inclusive of the 1440-minute
    /// following-noon boundary used only by the bounded historical repair.
    pub mask_off_minute: u16,
    /// Repair applied to the source pair, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<StrBoundaryRepair>,
}

/// A narrowly-scoped repair applied to a source boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrBoundaryRepair {
    /// Slot zero had a mask-off but encoded its continuing mask-on as zero.
    SlotZeroContinuation,
    /// The final source slot was still open on a completed historical day.
    HistoricalTrailingNoon,
}

/// Therapy boundaries decoded from one daily STR record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrTherapyDay {
    /// Zero-based EDF data-record index.
    pub record_index: usize,
    /// Device-local start of the record. ResMed STR records start at noon.
    pub local_noon: DeviceLocalDateTime,
    /// Raw digital value from the selected mask-event-count signal.
    pub mask_event_count: i16,
    /// Valid source-order mask-on/off pairs for this record.
    pub boundaries: Vec<StrTherapyBoundary>,
}

/// Fixed-size, privacy-safe counters for malformed or intentionally omitted
/// boundary slots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrBoundaryDiagnostics {
    /// Slots containing a positive offset above 1440. Non-positive values are
    /// the source's absent-value sentinel and are not offsets.
    pub out_of_range_slots: u32,
    /// Non-empty slots that could not form a positive-duration pair.
    pub invalid_pair_slots: u32,
    /// Current/future trailing mask-ons omitted rather than inventing an end.
    pub unfinished_non_historical_slots: u32,
    /// Complete pairs omitted because their end lies after the supplied
    /// device-local wall clock, or their record is in the future.
    pub future_boundary_slots: u32,
    /// Historical trailing mask-ons repaired to the following noon.
    pub repaired_historical_slots: u32,
    /// Days discarded because otherwise-valid intervals overlap or share a
    /// mask-on minute and therefore cannot seed unique sessions safely.
    pub ambiguous_days: u32,
    /// Days whose raw mask-event count differs from twice the retained interval
    /// count. The count is diagnostic only and never changes a boundary.
    pub mask_event_count_mismatch_days: u32,
}

/// Complete bounded result for one uncompressed root `STR.edf`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrBoundaryIndex {
    pub serial_verification: StrSerialVerification,
    pub selected_labels: StrSelectedSignalLabels,
    /// Every decoded STR record, including days without a valid boundary.
    pub days: Vec<StrTherapyDay>,
    pub diagnostics: StrBoundaryDiagnostics,
}

/// Semantic role of one required STR signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrSignalRole {
    MaskOn,
    MaskOff,
    MaskEvents,
}

impl fmt::Display for StrSignalRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MaskOn => "mask-on",
            Self::MaskOff => "mask-off",
            Self::MaskEvents => "mask-event-count",
        })
    }
}

/// Failure to decode a trustworthy STR boundary source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrDecodeError {
    FileTooLarge {
        limit: usize,
        actual: usize,
    },
    InvalidCurrentDeviceTime,
    EmptyExpectedSerial,
    Parse(ParseError),
    UnsupportedEdfPlus,
    TrailingData {
        bytes: usize,
    },
    InvalidRecordDuration,
    InvalidRecordStart,
    MissingSerial,
    SerialMismatch,
    AmbiguousSerial,
    MissingSignal(StrSignalRole),
    AmbiguousSignal(StrSignalRole),
    NonDigitalSignal(StrSignalRole),
    InvalidSamplesPerRecord {
        role: StrSignalRole,
        expected: &'static str,
        actual: usize,
    },
    MaskSampleCountMismatch {
        mask_on: usize,
        mask_off: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    DateRange,
}

impl fmt::Display for StrDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { limit, actual } => {
                write!(
                    formatter,
                    "STR EDF exceeds the {limit}-byte input limit ({actual} bytes)"
                )
            }
            Self::InvalidCurrentDeviceTime => {
                formatter.write_str("current device-local time is not a valid calendar time")
            }
            Self::EmptyExpectedSerial => {
                formatter.write_str("expected ResMed serial must not be empty")
            }
            Self::Parse(source) => write!(formatter, "could not parse bounded STR EDF: {source}"),
            Self::UnsupportedEdfPlus => {
                formatter.write_str("STR boundary decoding accepts plain EDF only")
            }
            Self::TrailingData { bytes } => {
                write!(
                    formatter,
                    "STR EDF has {bytes} trailing bytes after its declared records"
                )
            }
            Self::InvalidRecordDuration => {
                formatter.write_str("STR EDF records must each span exactly one day")
            }
            Self::InvalidRecordStart => {
                formatter.write_str("STR EDF must start at device-local noon")
            }
            Self::MissingSerial => {
                formatter.write_str("STR recording identity is missing its SRN token")
            }
            Self::SerialMismatch => formatter
                .write_str("STR recording identity does not match the selected ResMed card"),
            Self::AmbiguousSerial => {
                formatter.write_str("STR recording identity contains multiple SRN tokens")
            }
            Self::MissingSignal(role) => write!(formatter, "STR EDF is missing {role} data"),
            Self::AmbiguousSignal(role) => {
                write!(formatter, "STR EDF contains duplicate exact {role} signals")
            }
            Self::NonDigitalSignal(role) => {
                write!(
                    formatter,
                    "STR {role} signal is not a digital sampled signal"
                )
            }
            Self::InvalidSamplesPerRecord {
                role,
                expected,
                actual,
            } => write!(
                formatter,
                "STR {role} signal has {actual} samples per record; expected {expected}"
            ),
            Self::MaskSampleCountMismatch { mask_on, mask_off } => write!(
                formatter,
                "STR mask-on/off sample counts differ ({mask_on} versus {mask_off})"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not reserve capacity for {requested} {resource}"
            ),
            Self::DateRange => {
                formatter.write_str("STR record date exceeds the supported calendar range")
            }
        }
    }
}

impl error::Error for StrDecodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Parse(source) => Some(source),
            _ => None,
        }
    }
}

impl From<ParseError> for StrDecodeError {
    fn from(source: ParseError) -> Self {
        Self::Parse(source)
    }
}

/// Decode therapy boundaries from one complete, uncompressed root `STR.edf`.
///
/// Signal lookup exactly follows pinned OSCAR precedence: the case-sensitive
/// spaced spelling is tried first, then the compact spelling. The recording
/// must contain exactly one non-empty `SRN=` token equal to `expected_serial`;
/// otherwise the entire STR source is rejected so detail-file grouping can
/// fall back safely.
///
/// # Errors
///
/// Returns [`StrDecodeError`] for an over-limit, malformed, wrongly identified,
/// structurally incompatible, or non-daily EDF input.
pub fn decode_str(
    bytes: &[u8],
    expected_serial: &str,
    current_device_local_time: DeviceLocalDateTime,
) -> Result<StrBoundaryIndex, StrDecodeError> {
    decode_str_with_options(
        bytes,
        StrDecodeOptions {
            expected_serial: Some(expected_serial),
            current_device_local_time,
        },
    )
}

fn decode_str_with_options(
    bytes: &[u8],
    options: StrDecodeOptions<'_>,
) -> Result<StrBoundaryIndex, StrDecodeError> {
    if bytes.len() > RESMED_STR_MAX_FILE_BYTES {
        return Err(StrDecodeError::FileTooLarge {
            limit: RESMED_STR_MAX_FILE_BYTES,
            actual: bytes.len(),
        });
    }
    if !valid_local_datetime(&options.current_device_local_time) {
        return Err(StrDecodeError::InvalidCurrentDeviceTime);
    }
    if options.expected_serial.is_some_and(str::is_empty) {
        return Err(StrDecodeError::EmptyExpectedSerial);
    }

    let parser = Parser::new(STR_LIMITS);
    let header = parser.parse_header(bytes)?;
    validate_header_shape(&header)?;
    let selected = select_signal_headers(&header.signals)?;
    validate_selected_header_shapes(&header.signals, selected)?;
    let serial_verification = verify_serial(&header.recording_id, options.expected_serial)?;
    let parsed = parser.parse(bytes)?;

    if parsed.trailing_data_bytes() != 0 {
        return Err(StrDecodeError::TrailingData {
            bytes: parsed.trailing_data_bytes(),
        });
    }

    let signals = selected.resolve(&parsed)?;
    validate_decoded_signals(signals)?;

    decode_records(
        &parsed,
        signals,
        selected.labels(),
        serial_verification,
        &options.current_device_local_time,
    )
}

#[derive(Debug, Clone, Copy)]
struct SelectedSignalIndices {
    mask_on: usize,
    mask_off: usize,
    mask_events: usize,
    mask_on_style: StrSignalLabelStyle,
    mask_off_style: StrSignalLabelStyle,
    mask_events_style: StrSignalLabelStyle,
}

impl SelectedSignalIndices {
    const fn labels(self) -> StrSelectedSignalLabels {
        StrSelectedSignalLabels {
            mask_on: self.mask_on_style,
            mask_off: self.mask_off_style,
            mask_events: self.mask_events_style,
        }
    }

    fn resolve<'a>(self, parsed: &'a EdfFile) -> Result<SelectedSignals<'a>, StrDecodeError> {
        Ok(SelectedSignals {
            mask_on: parsed
                .signal(self.mask_on)
                .ok_or(StrDecodeError::MissingSignal(StrSignalRole::MaskOn))?,
            mask_off: parsed
                .signal(self.mask_off)
                .ok_or(StrDecodeError::MissingSignal(StrSignalRole::MaskOff))?,
            mask_events: parsed
                .signal(self.mask_events)
                .ok_or(StrDecodeError::MissingSignal(StrSignalRole::MaskEvents))?,
            mask_on_index: self.mask_on,
            mask_off_index: self.mask_off,
            mask_events_index: self.mask_events,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct SelectedSignals<'a> {
    mask_on: &'a Signal,
    mask_off: &'a Signal,
    mask_events: &'a Signal,
    mask_on_index: usize,
    mask_off_index: usize,
    mask_events_index: usize,
}

fn validate_header_shape(header: &opap_edf::EdfHeader) -> Result<(), StrDecodeError> {
    if header.is_continuous() || header.is_discontinuous() {
        return Err(StrDecodeError::UnsupportedEdfPlus);
    }
    if header.record_duration_seconds.to_bits() != SECONDS_PER_DAY.to_bits() {
        return Err(StrDecodeError::InvalidRecordDuration);
    }
    if header.start.hour != 12 || header.start.minute != 0 || header.start.second != 0 {
        return Err(StrDecodeError::InvalidRecordStart);
    }
    Ok(())
}

fn select_signal_headers(
    signals: &[opap_edf::SignalHeader],
) -> Result<SelectedSignalIndices, StrDecodeError> {
    let (mask_on, mask_on_style) =
        select_signal(signals, "Mask On", "MaskOn", StrSignalRole::MaskOn)?;
    let (mask_off, mask_off_style) =
        select_signal(signals, "Mask Off", "MaskOff", StrSignalRole::MaskOff)?;
    let (mask_events, mask_events_style) = select_signal(
        signals,
        "Mask Events",
        "MaskEvents",
        StrSignalRole::MaskEvents,
    )?;
    Ok(SelectedSignalIndices {
        mask_on,
        mask_off,
        mask_events,
        mask_on_style,
        mask_off_style,
        mask_events_style,
    })
}

fn select_signal(
    signals: &[opap_edf::SignalHeader],
    spaced: &str,
    compact: &str,
    role: StrSignalRole,
) -> Result<(usize, StrSignalLabelStyle), StrDecodeError> {
    let spaced_matches = signals
        .iter()
        .enumerate()
        .filter(|(_, signal)| signal.label == spaced);
    let mut spaced_matches = spaced_matches.map(|(index, _)| index);
    if let Some(index) = spaced_matches.next() {
        if spaced_matches.next().is_some() {
            return Err(StrDecodeError::AmbiguousSignal(role));
        }
        return Ok((index, StrSignalLabelStyle::Spaced));
    }

    let compact_matches = signals
        .iter()
        .enumerate()
        .filter(|(_, signal)| signal.label == compact);
    let mut compact_matches = compact_matches.map(|(index, _)| index);
    let index = compact_matches
        .next()
        .ok_or(StrDecodeError::MissingSignal(role))?;
    if compact_matches.next().is_some() {
        return Err(StrDecodeError::AmbiguousSignal(role));
    }
    Ok((index, StrSignalLabelStyle::Compact))
}

fn validate_selected_header_shapes(
    headers: &[opap_edf::SignalHeader],
    selected: SelectedSignalIndices,
) -> Result<(), StrDecodeError> {
    let mask_on_count = headers
        .get(selected.mask_on)
        .ok_or(StrDecodeError::MissingSignal(StrSignalRole::MaskOn))?
        .samples_per_record;
    let mask_off_count = headers
        .get(selected.mask_off)
        .ok_or(StrDecodeError::MissingSignal(StrSignalRole::MaskOff))?
        .samples_per_record;
    let mask_events_count = headers
        .get(selected.mask_events)
        .ok_or(StrDecodeError::MissingSignal(StrSignalRole::MaskEvents))?
        .samples_per_record;
    validate_sample_counts(mask_on_count, mask_off_count, mask_events_count)
}

fn validate_decoded_signals(signals: SelectedSignals<'_>) -> Result<(), StrDecodeError> {
    let mask_on_count =
        validate_digital_signal(signals.mask_on, StrSignalRole::MaskOn, "between 1 and 128")?;
    let mask_off_count = validate_digital_signal(
        signals.mask_off,
        StrSignalRole::MaskOff,
        "between 1 and 128",
    )?;
    let event_count =
        validate_digital_signal(signals.mask_events, StrSignalRole::MaskEvents, "exactly 1")?;
    validate_sample_counts(mask_on_count, mask_off_count, event_count)
}

fn validate_sample_counts(
    mask_on_count: usize,
    mask_off_count: usize,
    event_count: usize,
) -> Result<(), StrDecodeError> {
    if mask_on_count == 0 {
        return Err(StrDecodeError::InvalidSamplesPerRecord {
            role: StrSignalRole::MaskOn,
            expected: "between 1 and 128",
            actual: mask_on_count,
        });
    }
    if mask_off_count == 0 {
        return Err(StrDecodeError::InvalidSamplesPerRecord {
            role: StrSignalRole::MaskOff,
            expected: "between 1 and 128",
            actual: mask_off_count,
        });
    }
    if mask_on_count > MAX_MASK_SLOTS_PER_RECORD {
        return Err(StrDecodeError::InvalidSamplesPerRecord {
            role: StrSignalRole::MaskOn,
            expected: "between 1 and 128",
            actual: mask_on_count,
        });
    }
    if mask_off_count > MAX_MASK_SLOTS_PER_RECORD {
        return Err(StrDecodeError::InvalidSamplesPerRecord {
            role: StrSignalRole::MaskOff,
            expected: "between 1 and 128",
            actual: mask_off_count,
        });
    }
    if mask_on_count != mask_off_count {
        return Err(StrDecodeError::MaskSampleCountMismatch {
            mask_on: mask_on_count,
            mask_off: mask_off_count,
        });
    }
    if event_count != 1 {
        return Err(StrDecodeError::InvalidSamplesPerRecord {
            role: StrSignalRole::MaskEvents,
            expected: "exactly 1",
            actual: event_count,
        });
    }
    Ok(())
}

fn validate_digital_signal(
    signal: &Signal,
    role: StrSignalRole,
    expected: &'static str,
) -> Result<usize, StrDecodeError> {
    if signal.digital_samples().is_none() {
        return Err(StrDecodeError::NonDigitalSignal(role));
    }
    let count = signal.header.samples_per_record;
    if count == 0 {
        return Err(StrDecodeError::InvalidSamplesPerRecord {
            role,
            expected,
            actual: count,
        });
    }
    Ok(count)
}

fn verify_serial(
    recording_id: &str,
    expected_serial: Option<&str>,
) -> Result<StrSerialVerification, StrDecodeError> {
    let Some(expected) = expected_serial else {
        return Ok(StrSerialVerification::NotRequested);
    };
    let mut serials = recording_serials(recording_id);
    let actual = serials.next();
    if serials.next().is_some() {
        return Err(StrDecodeError::AmbiguousSerial);
    }
    match actual {
        Some(actual) if actual == expected => Ok(StrSerialVerification::Verified),
        Some(_) => Err(StrDecodeError::SerialMismatch),
        None => Err(StrDecodeError::MissingSerial),
    }
}

fn recording_serials(recording_id: &str) -> impl Iterator<Item = &str> {
    recording_id
        .split_ascii_whitespace()
        .filter_map(|token| token.strip_prefix("SRN="))
        .filter(|serial| !serial.is_empty())
}

fn decode_records(
    parsed: &EdfFile,
    signals: SelectedSignals<'_>,
    selected_labels: StrSelectedSignalLabels,
    serial_verification: StrSerialVerification,
    current: &DeviceLocalDateTime,
) -> Result<StrBoundaryIndex, StrDecodeError> {
    let mut days = Vec::new();
    days.try_reserve_exact(parsed.record_count()).map_err(|_| {
        StrDecodeError::AllocationFailed {
            resource: "STR daily records",
            requested: parsed.record_count(),
        }
    })?;

    let start = resmed_header_start(parsed.header().start)?;
    let start_day_number = days_from_civil(start.year, start.month, start.day);
    let current_calendar_day = days_from_civil(current.year, current.month, current.day);
    let current_resmed_day_number = current_calendar_day
        .checked_sub(i64::from(current.hour < 12))
        .ok_or(StrDecodeError::DateRange)?;
    let current_resmed_day_elapsed_millis = current_resmed_day_elapsed_millis(current);
    let mut diagnostics = StrBoundaryDiagnostics::default();

    for record in parsed.records() {
        let record_index_i64 =
            i64::try_from(record.index()).map_err(|_| StrDecodeError::DateRange)?;
        let record_day_number = start_day_number
            .checked_add(record_index_i64)
            .ok_or(StrDecodeError::DateRange)?;
        let (year, month, day) =
            civil_from_days(record_day_number).ok_or(StrDecodeError::DateRange)?;
        let local_noon = DeviceLocalDateTime {
            year,
            month,
            day,
            hour: 12,
            minute: 0,
            second: 0,
            millisecond: 0,
        };

        let mask_on = record
            .digital_samples(signals.mask_on_index)
            .ok_or(StrDecodeError::NonDigitalSignal(StrSignalRole::MaskOn))?;
        let mask_off = record
            .digital_samples(signals.mask_off_index)
            .ok_or(StrDecodeError::NonDigitalSignal(StrSignalRole::MaskOff))?;
        let mask_events = record
            .digital_samples(signals.mask_events_index)
            .and_then(|samples| samples.first())
            .copied()
            .ok_or(StrDecodeError::NonDigitalSignal(StrSignalRole::MaskEvents))?;

        let last_populated_slot = mask_on
            .iter()
            .zip(mask_off)
            .rposition(|(&on, &off)| on > 0 || off > 0);
        let mut boundaries = Vec::new();
        let mut repaired_historical_this_day = 0_u32;
        boundaries.try_reserve_exact(mask_on.len()).map_err(|_| {
            StrDecodeError::AllocationFailed {
                resource: "STR therapy boundaries",
                requested: mask_on.len(),
            }
        })?;

        for (slot, (&on, &off)) in mask_on.iter().zip(mask_off).enumerate() {
            let source_slot = u16::try_from(slot).expect("mask slot bound fits u16");
            if on > MINUTES_PER_DAY || off > MINUTES_PER_DAY {
                diagnostics.out_of_range_slots = diagnostics.out_of_range_slots.saturating_add(1);
                continue;
            }

            if slot == 0 && on == 0 && off > 0 {
                if boundary_ends_in_future(
                    record_day_number,
                    off,
                    current_resmed_day_number,
                    current_resmed_day_elapsed_millis,
                ) {
                    diagnostics.future_boundary_slots =
                        diagnostics.future_boundary_slots.saturating_add(1);
                    continue;
                }
                boundaries.push(StrTherapyBoundary {
                    source_slot,
                    source_mask_on_value: Some(on),
                    source_mask_off_value: Some(off),
                    mask_on_minute: 0,
                    mask_off_minute: u16::try_from(off).expect("positive minute fits u16"),
                    repair: Some(StrBoundaryRepair::SlotZeroContinuation),
                });
                continue;
            }

            if on > 0 && on < MINUTES_PER_DAY && off <= 0 && last_populated_slot == Some(slot) {
                if record_day_number < current_resmed_day_number {
                    boundaries.push(StrTherapyBoundary {
                        source_slot,
                        source_mask_on_value: Some(on),
                        source_mask_off_value: Some(off),
                        mask_on_minute: u16::try_from(on).expect("positive minute fits u16"),
                        mask_off_minute: u16::try_from(MINUTES_PER_DAY)
                            .expect("minute bound fits u16"),
                        repair: Some(StrBoundaryRepair::HistoricalTrailingNoon),
                    });
                    repaired_historical_this_day = repaired_historical_this_day.saturating_add(1);
                } else {
                    diagnostics.unfinished_non_historical_slots = diagnostics
                        .unfinished_non_historical_slots
                        .saturating_add(1);
                }
                continue;
            }

            if on > 0 && off > on {
                if boundary_ends_in_future(
                    record_day_number,
                    off,
                    current_resmed_day_number,
                    current_resmed_day_elapsed_millis,
                ) {
                    diagnostics.future_boundary_slots =
                        diagnostics.future_boundary_slots.saturating_add(1);
                    continue;
                }
                boundaries.push(StrTherapyBoundary {
                    source_slot,
                    source_mask_on_value: Some(on),
                    source_mask_off_value: Some(off),
                    mask_on_minute: u16::try_from(on).expect("positive minute fits u16"),
                    mask_off_minute: u16::try_from(off).expect("positive minute fits u16"),
                    repair: None,
                });
            } else if on > 0 || off > 0 {
                diagnostics.invalid_pair_slots = diagnostics.invalid_pair_slots.saturating_add(1);
            }
        }

        if has_ambiguous_intervals(&boundaries) {
            boundaries.clear();
            diagnostics.ambiguous_days = diagnostics.ambiguous_days.saturating_add(1);
        } else {
            diagnostics.repaired_historical_slots = diagnostics
                .repaired_historical_slots
                .saturating_add(repaired_historical_this_day);
        }
        let retained_event_count = boundaries.len().saturating_mul(2);
        if mask_events >= 0 && usize::try_from(mask_events).ok() != Some(retained_event_count) {
            diagnostics.mask_event_count_mismatch_days =
                diagnostics.mask_event_count_mismatch_days.saturating_add(1);
        }

        days.push(StrTherapyDay {
            record_index: record.index(),
            local_noon,
            mask_event_count: mask_events,
            boundaries,
        });
    }

    Ok(StrBoundaryIndex {
        serial_verification,
        selected_labels,
        days,
        diagnostics,
    })
}

fn boundary_ends_in_future(
    record_day_number: i64,
    mask_off_minute: i16,
    current_resmed_day_number: i64,
    current_resmed_day_elapsed_millis: i64,
) -> bool {
    record_day_number > current_resmed_day_number
        || (record_day_number == current_resmed_day_number
            && i64::from(mask_off_minute) * 60_000 > current_resmed_day_elapsed_millis)
}

fn has_ambiguous_intervals(boundaries: &[StrTherapyBoundary]) -> bool {
    // Minute granularity and the 0..=1440 offset bound let us check overlap in
    // O(boundaries + minutes-per-day), independent of source ordering.
    let mut occupied = [false; MINUTES_PER_DAY as usize];
    let mut starts = [false; MINUTES_PER_DAY as usize + 1];
    for boundary in boundaries {
        let start = usize::from(boundary.mask_on_minute);
        let end = usize::from(boundary.mask_off_minute);
        if starts[start] {
            return true;
        }
        starts[start] = true;
        for minute in &mut occupied[start..end] {
            if *minute {
                return true;
            }
            *minute = true;
        }
    }
    false
}

fn current_resmed_day_elapsed_millis(current: &DeviceLocalDateTime) -> i64 {
    let hour_after_noon = if current.hour >= 12 {
        current.hour - 12
    } else {
        current.hour + 12
    };
    i64::from(hour_after_noon) * 3_600_000
        + i64::from(current.minute) * 60_000
        + i64::from(current.second) * 1_000
        + i64::from(current.millisecond)
}

fn resmed_header_start(
    start: opap_edf::EdfDateTime,
) -> Result<DeviceLocalDateTime, StrDecodeError> {
    let year = if start.year < 2000 {
        start
            .year
            .checked_add(100)
            .ok_or(StrDecodeError::DateRange)?
    } else {
        start.year
    };
    let value = DeviceLocalDateTime {
        year,
        month: start.month,
        day: start.day,
        hour: start.hour,
        minute: start.minute,
        second: start.second,
        millisecond: 0,
    };
    valid_local_datetime(&value)
        .then_some(value)
        .ok_or(StrDecodeError::DateRange)
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

    fn decode_str(
        bytes: &[u8],
        options: StrDecodeOptions<'_>,
    ) -> Result<StrBoundaryIndex, StrDecodeError> {
        super::decode_str_with_options(bytes, options)
    }

    #[test]
    fn legacy_serialized_boundary_defaults_new_source_samples() {
        let boundary: StrTherapyBoundary = serde_json::from_value(serde_json::json!({
            "source_slot": 0,
            "mask_on_minute": 100,
            "mask_off_minute": 200
        }))
        .expect("legacy boundary remains readable");
        assert_eq!(boundary.source_mask_on_value, None);
        assert_eq!(boundary.source_mask_off_value, None);
        assert_eq!(boundary.repair, None);
    }

    #[derive(Clone)]
    struct SignalFixture<'a> {
        label: &'a str,
        samples_per_record: usize,
        samples: Vec<i16>,
        physical_minimum: i32,
        physical_maximum: i32,
        digital_minimum: i32,
        digital_maximum: i32,
    }

    impl<'a> SignalFixture<'a> {
        fn new(label: &'a str, samples_per_record: usize, samples: &[i16]) -> Self {
            Self {
                label,
                samples_per_record,
                samples: samples.to_vec(),
                physical_minimum: -32_768,
                physical_maximum: 32_767,
                digital_minimum: -32_768,
                digital_maximum: 32_767,
            }
        }

        fn calibration(
            mut self,
            physical_minimum: i32,
            physical_maximum: i32,
            digital_minimum: i32,
            digital_maximum: i32,
        ) -> Self {
            self.physical_minimum = physical_minimum;
            self.physical_maximum = physical_maximum;
            self.digital_minimum = digital_minimum;
            self.digital_maximum = digital_maximum;
            self
        }
    }

    fn current(year: u16, month: u8, day: u8) -> DeviceLocalDateTime {
        at(year, month, day, 23, 59, 59, 999)
    }

    fn at(
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        millisecond: u16,
    ) -> DeviceLocalDateTime {
        DeviceLocalDateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond,
        }
    }

    fn options<'a>(
        expected_serial: Option<&'a str>,
        now: DeviceLocalDateTime,
    ) -> StrDecodeOptions<'a> {
        StrDecodeOptions {
            expected_serial,
            current_device_local_time: now,
        }
    }

    fn field(value: &str, width: usize) -> Vec<u8> {
        assert!(value.len() <= width);
        let mut output = vec![b' '; width];
        output[..value.len()].copy_from_slice(value.as_bytes());
        output
    }

    fn synthetic_str(
        signals: &[SignalFixture<'_>],
        record_count: usize,
        recording_id: &str,
        start: &str,
        duration: &str,
        reserved: &str,
    ) -> Vec<u8> {
        assert_eq!(start.len(), 16);
        let header_bytes = 256 + signals.len() * 256;
        let mut bytes = Vec::new();
        bytes.extend(field("0", 8));
        bytes.extend(field("patient", 80));
        bytes.extend(field(recording_id, 80));
        bytes.extend_from_slice(start.as_bytes());
        bytes.extend(field(&header_bytes.to_string(), 8));
        bytes.extend(field(reserved, 44));
        bytes.extend(field(&record_count.to_string(), 8));
        bytes.extend(field(duration, 8));
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
        for signal in signals {
            bytes.extend(field(&signal.physical_minimum.to_string(), 8));
        }
        for signal in signals {
            bytes.extend(field(&signal.physical_maximum.to_string(), 8));
        }
        for signal in signals {
            bytes.extend(field(&signal.digital_minimum.to_string(), 8));
        }
        for signal in signals {
            bytes.extend(field(&signal.digital_maximum.to_string(), 8));
        }
        for _ in signals {
            bytes.extend(field("", 80));
        }
        for signal in signals {
            bytes.extend(field(&signal.samples_per_record.to_string(), 8));
        }
        for _ in signals {
            bytes.extend(field("", 32));
        }
        assert_eq!(bytes.len(), header_bytes);

        for record in 0..record_count {
            for signal in signals {
                assert_eq!(
                    signal.samples.len(),
                    record_count * signal.samples_per_record
                );
                let start = record * signal.samples_per_record;
                let end = start + signal.samples_per_record;
                for sample in &signal.samples[start..end] {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
            }
        }
        bytes
    }

    fn standard_str(
        mask_on: &[i16],
        mask_off: &[i16],
        event_counts: &[i16],
        record_count: usize,
    ) -> Vec<u8> {
        let slots = mask_on.len() / record_count;
        synthetic_str(
            &[
                SignalFixture::new("Mask On", slots, mask_on),
                SignalFixture::new("Mask Off", slots, mask_off),
                SignalFixture::new("Mask Events", 1, event_counts),
            ],
            record_count,
            "ResMed SRN=serial-123",
            "01.01.2612.00.00",
            "86400",
            "",
        )
    }

    #[test]
    fn decodes_raw_boundaries_and_bounds_repairs_by_device_day() {
        let bytes = synthetic_str(
            &[
                SignalFixture::new(
                    "Mask On",
                    3,
                    &[
                        0, 100, 300, // historical
                        60, 400, 0, // current
                    ],
                )
                .calibration(0, 1, -32_768, 32_767),
                SignalFixture::new(
                    "Mask Off",
                    3,
                    &[
                        50, 200, 0, // slot-zero continuation + trailing repair
                        120, 0, 0, // completed + unfinished current
                    ],
                )
                .calibration(10, 20, -100, 100),
                SignalFixture::new("Mask Events", 1, &[5, 3]).calibration(0, 100, -1, 1),
            ],
            2,
            "ResMed SRN=serial-123",
            "01.01.2612.00.00",
            "86400",
            "",
        );

        let decoded = decode_str(&bytes, options(Some("serial-123"), current(2026, 1, 2))).unwrap();

        assert_eq!(decoded.serial_verification, StrSerialVerification::Verified);
        assert_eq!(
            decoded.selected_labels,
            StrSelectedSignalLabels {
                mask_on: StrSignalLabelStyle::Spaced,
                mask_off: StrSignalLabelStyle::Spaced,
                mask_events: StrSignalLabelStyle::Spaced,
            }
        );
        assert_eq!(decoded.days.len(), 2);
        assert_eq!(decoded.days[0].mask_event_count, 5);
        assert_eq!(
            decoded.days[0].boundaries,
            vec![
                StrTherapyBoundary {
                    source_slot: 0,
                    source_mask_on_value: Some(0),
                    source_mask_off_value: Some(50),
                    mask_on_minute: 0,
                    mask_off_minute: 50,
                    repair: Some(StrBoundaryRepair::SlotZeroContinuation),
                },
                StrTherapyBoundary {
                    source_slot: 1,
                    source_mask_on_value: Some(100),
                    source_mask_off_value: Some(200),
                    mask_on_minute: 100,
                    mask_off_minute: 200,
                    repair: None,
                },
                StrTherapyBoundary {
                    source_slot: 2,
                    source_mask_on_value: Some(300),
                    source_mask_off_value: Some(0),
                    mask_on_minute: 300,
                    mask_off_minute: 1440,
                    repair: Some(StrBoundaryRepair::HistoricalTrailingNoon),
                },
            ]
        );
        assert_eq!(decoded.days[1].mask_event_count, 3);
        assert_eq!(
            decoded.days[1].boundaries,
            vec![StrTherapyBoundary {
                source_slot: 0,
                source_mask_on_value: Some(60),
                source_mask_off_value: Some(120),
                mask_on_minute: 60,
                mask_off_minute: 120,
                repair: None,
            }]
        );
        assert_eq!(
            decoded.diagnostics,
            StrBoundaryDiagnostics {
                repaired_historical_slots: 1,
                unfinished_non_historical_slots: 1,
                mask_event_count_mismatch_days: 2,
                ..StrBoundaryDiagnostics::default()
            }
        );
    }

    #[test]
    fn spaced_labels_take_precedence_over_compact_labels() {
        let bytes = synthetic_str(
            &[
                SignalFixture::new("MaskOn", 1, &[700]),
                SignalFixture::new("MaskOff", 1, &[800]),
                SignalFixture::new("MaskEvents", 1, &[99]),
                SignalFixture::new("Mask On", 1, &[100]),
                SignalFixture::new("Mask Off", 1, &[200]),
                SignalFixture::new("Mask Events", 1, &[2]),
            ],
            1,
            "SRN=serial-123",
            "01.01.2612.00.00",
            "86400",
            "",
        );

        let decoded = decode_str(&bytes, options(Some("serial-123"), current(2027, 1, 1))).unwrap();
        assert_eq!(decoded.days[0].mask_event_count, 2);
        assert_eq!(decoded.days[0].boundaries[0].mask_on_minute, 100);
        assert_eq!(decoded.days[0].boundaries[0].mask_off_minute, 200);
        assert_eq!(decoded.selected_labels.mask_on, StrSignalLabelStyle::Spaced);
    }

    #[test]
    fn compact_labels_are_supported_as_exact_case_sensitive_fallbacks() {
        let bytes = synthetic_str(
            &[
                SignalFixture::new("MaskOn", 1, &[100]),
                SignalFixture::new("MaskOff", 1, &[200]),
                SignalFixture::new("MaskEvents", 1, &[2]),
            ],
            1,
            "SRN=serial-123",
            "01.01.2612.00.00",
            "86400",
            "",
        );
        let decoded = decode_str(&bytes, options(Some("serial-123"), current(2027, 1, 1))).unwrap();
        assert_eq!(
            decoded.selected_labels,
            StrSelectedSignalLabels {
                mask_on: StrSignalLabelStyle::Compact,
                mask_off: StrSignalLabelStyle::Compact,
                mask_events: StrSignalLabelStyle::Compact,
            }
        );

        let wrong_case = synthetic_str(
            &[
                SignalFixture::new("mask on", 1, &[100]),
                SignalFixture::new("Mask Off", 1, &[200]),
                SignalFixture::new("Mask Events", 1, &[2]),
            ],
            1,
            "SRN=serial-123",
            "01.01.2612.00.00",
            "86400",
            "",
        );
        assert_eq!(
            decode_str(
                &wrong_case,
                options(Some("serial-123"), current(2027, 1, 1))
            ),
            Err(StrDecodeError::MissingSignal(StrSignalRole::MaskOn))
        );
    }

    #[test]
    fn serial_verification_is_exact_private_and_optionally_explicitly_skipped() {
        let missing = synthetic_str(
            &[
                SignalFixture::new("Mask On", 1, &[100]),
                SignalFixture::new("Mask Off", 1, &[200]),
                SignalFixture::new("Mask Events", 1, &[2]),
            ],
            1,
            "ResMed recording",
            "01.01.2612.00.00",
            "86400",
            "",
        );
        let mismatch = synthetic_str(
            &[
                SignalFixture::new("Mask On", 1, &[100]),
                SignalFixture::new("Mask Off", 1, &[200]),
                SignalFixture::new("Mask Events", 1, &[2]),
            ],
            1,
            "SRN=other-private-serial",
            "01.01.2612.00.00",
            "86400",
            "",
        );

        assert_eq!(
            decode_str(
                &missing,
                options(Some("private-expected"), current(2027, 1, 1))
            ),
            Err(StrDecodeError::MissingSerial)
        );
        let mismatch_error = decode_str(
            &mismatch,
            options(Some("private-expected"), current(2027, 1, 1)),
        )
        .unwrap_err();
        assert_eq!(mismatch_error, StrDecodeError::SerialMismatch);
        let display = mismatch_error.to_string();
        assert!(!display.contains("private-expected"));
        assert!(!display.contains("other-private-serial"));

        let ambiguous = synthetic_str(
            &[
                SignalFixture::new("Mask On", 1, &[100]),
                SignalFixture::new("Mask Off", 1, &[200]),
                SignalFixture::new("Mask Events", 1, &[2]),
            ],
            1,
            "SRN=private-expected SRN=private-expected",
            "01.01.2612.00.00",
            "86400",
            "",
        );
        assert_eq!(
            super::decode_str(&ambiguous, "private-expected", current(2027, 1, 1)),
            Err(StrDecodeError::AmbiguousSerial)
        );

        let mut malformed_mismatch = mismatch.clone();
        malformed_mismatch.truncate(256 + 3 * 256);
        assert_eq!(
            super::decode_str(&malformed_mismatch, "private-expected", current(2027, 1, 1)),
            Err(StrDecodeError::SerialMismatch)
        );

        let unverified = decode_str(&missing, options(None, current(2027, 1, 1))).unwrap();
        assert_eq!(
            unverified.serial_verification,
            StrSerialVerification::NotRequested
        );
        assert_eq!(
            decode_str(&missing, options(Some(""), current(2027, 1, 1))),
            Err(StrDecodeError::EmptyExpectedSerial)
        );
    }

    #[test]
    fn sample_shapes_must_match_and_event_count_has_one_raw_sample() {
        let mismatched = synthetic_str(
            &[
                SignalFixture::new("Mask On", 2, &[100, 0]),
                SignalFixture::new("Mask Off", 1, &[200]),
                SignalFixture::new("Mask Events", 1, &[2]),
            ],
            1,
            "SRN=serial-123",
            "01.01.2612.00.00",
            "86400",
            "",
        );
        assert_eq!(
            decode_str(
                &mismatched,
                options(Some("serial-123"), current(2027, 1, 1))
            ),
            Err(StrDecodeError::MaskSampleCountMismatch {
                mask_on: 2,
                mask_off: 1,
            })
        );

        let event_samples = synthetic_str(
            &[
                SignalFixture::new("Mask On", 1, &[100]),
                SignalFixture::new("Mask Off", 1, &[200]),
                SignalFixture::new("Mask Events", 2, &[2, 3]),
            ],
            1,
            "SRN=serial-123",
            "01.01.2612.00.00",
            "86400",
            "",
        );
        assert_eq!(
            decode_str(
                &event_samples,
                options(Some("serial-123"), current(2027, 1, 1))
            ),
            Err(StrDecodeError::InvalidSamplesPerRecord {
                role: StrSignalRole::MaskEvents,
                expected: "exactly 1",
                actual: 2,
            })
        );
    }

    #[test]
    fn duplicate_exact_selected_labels_are_rejected_before_payload_decoding() {
        let bytes = synthetic_str(
            &[
                SignalFixture::new("Mask On", 1, &[100]),
                SignalFixture::new("Mask On", 1, &[700]),
                SignalFixture::new("Mask Off", 1, &[200]),
                SignalFixture::new("Mask Events", 1, &[2]),
            ],
            1,
            "SRN=serial-123",
            "01.01.2612.00.00",
            "86400",
            "",
        );
        assert_eq!(
            decode_str(&bytes, options(Some("serial-123"), current(2027, 1, 1))),
            Err(StrDecodeError::AmbiguousSignal(StrSignalRole::MaskOn))
        );
    }

    #[test]
    fn invalid_offsets_and_non_trailing_incomplete_pairs_are_omitted() {
        let bytes = standard_str(
            &[100, 200, -1, 1439, 1500, 300],
            &[0, 250, 400, 1440, 1600, 200],
            &[10],
            1,
        );
        let decoded = decode_str(&bytes, options(Some("serial-123"), current(2027, 1, 1))).unwrap();

        assert_eq!(
            decoded.days[0].boundaries,
            vec![
                StrTherapyBoundary {
                    source_slot: 1,
                    source_mask_on_value: Some(200),
                    source_mask_off_value: Some(250),
                    mask_on_minute: 200,
                    mask_off_minute: 250,
                    repair: None,
                },
                StrTherapyBoundary {
                    source_slot: 3,
                    source_mask_on_value: Some(1439),
                    source_mask_off_value: Some(1440),
                    mask_on_minute: 1439,
                    mask_off_minute: 1440,
                    repair: None,
                },
            ]
        );
        assert_eq!(decoded.diagnostics.out_of_range_slots, 1);
        assert_eq!(decoded.diagnostics.invalid_pair_slots, 3);
        assert_eq!(decoded.diagnostics.repaired_historical_slots, 0);
    }

    #[test]
    fn unused_negative_sentinel_slots_do_not_hide_a_historical_trailing_mask_on() {
        let bytes = standard_str(&[100, -1, -1], &[0, -1, -1], &[2], 1);
        let decoded = decode_str(&bytes, options(Some("serial-123"), current(2026, 1, 2))).unwrap();
        assert_eq!(
            decoded.days[0].boundaries,
            vec![StrTherapyBoundary {
                source_slot: 0,
                source_mask_on_value: Some(100),
                source_mask_off_value: Some(0),
                mask_on_minute: 100,
                mask_off_minute: 1440,
                repair: Some(StrBoundaryRepair::HistoricalTrailingNoon),
            }]
        );
        assert_eq!(
            decoded.diagnostics,
            StrBoundaryDiagnostics {
                repaired_historical_slots: 1,
                ..StrBoundaryDiagnostics::default()
            }
        );
    }

    #[test]
    fn current_resmed_day_starts_at_noon_and_never_gains_future_usage() {
        let unfinished = standard_str(&[100], &[0], &[1], 1);
        let before_noon = decode_str(
            &unfinished,
            options(Some("serial-123"), at(2026, 1, 2, 8, 0, 0, 0)),
        )
        .unwrap();
        assert!(before_noon.days[0].boundaries.is_empty());
        assert_eq!(before_noon.diagnostics.unfinished_non_historical_slots, 1);
        assert_eq!(before_noon.diagnostics.repaired_historical_slots, 0);

        let at_next_noon = decode_str(
            &unfinished,
            options(Some("serial-123"), at(2026, 1, 2, 12, 0, 0, 0)),
        )
        .unwrap();
        assert_eq!(
            at_next_noon.days[0].boundaries[0],
            StrTherapyBoundary {
                source_slot: 0,
                source_mask_on_value: Some(100),
                source_mask_off_value: Some(0),
                mask_on_minute: 100,
                mask_off_minute: 1440,
                repair: Some(StrBoundaryRepair::HistoricalTrailingNoon),
            }
        );

        let complete = standard_str(&[30, 70], &[60, 90], &[4], 1);
        let one_pm = decode_str(
            &complete,
            options(Some("serial-123"), at(2026, 1, 1, 13, 0, 0, 0)),
        )
        .unwrap();
        assert_eq!(
            one_pm.days[0].boundaries,
            vec![StrTherapyBoundary {
                source_slot: 0,
                source_mask_on_value: Some(30),
                source_mask_off_value: Some(60),
                mask_on_minute: 30,
                mask_off_minute: 60,
                repair: None,
            }]
        );
        assert_eq!(one_pm.diagnostics.future_boundary_slots, 1);

        let current_slot_zero = standard_str(&[0], &[120], &[2], 1);
        let current_slot_zero = decode_str(
            &current_slot_zero,
            options(Some("serial-123"), at(2026, 1, 1, 13, 0, 0, 0)),
        )
        .unwrap();
        assert!(current_slot_zero.days[0].boundaries.is_empty());
        assert_eq!(current_slot_zero.diagnostics.future_boundary_slots, 1);

        let future_record = standard_str(&[0], &[60], &[2], 1);
        let future = decode_str(
            &future_record,
            options(Some("serial-123"), at(2025, 12, 31, 23, 59, 0, 0)),
        )
        .unwrap();
        assert!(future.days[0].boundaries.is_empty());
        assert_eq!(future.diagnostics.future_boundary_slots, 1);
    }

    #[test]
    fn overlapping_or_duplicate_mask_on_intervals_make_the_day_ambiguous() {
        let overlap = standard_str(&[100, 150], &[200, 250], &[4], 1);
        let decoded =
            decode_str(&overlap, options(Some("serial-123"), current(2027, 1, 1))).unwrap();
        assert!(decoded.days[0].boundaries.is_empty());
        assert_eq!(decoded.diagnostics.ambiguous_days, 1);
        assert_eq!(decoded.diagnostics.mask_event_count_mismatch_days, 1);

        let duplicate = standard_str(&[100, 100], &[150, 200], &[4], 1);
        let decoded =
            decode_str(&duplicate, options(Some("serial-123"), current(2027, 1, 1))).unwrap();
        assert!(decoded.days[0].boundaries.is_empty());
        assert_eq!(decoded.diagnostics.ambiguous_days, 1);
    }

    #[test]
    fn maximum_mask_slot_shape_uses_bounded_linear_ambiguity_detection() {
        let mask_on = (1..=MAX_MASK_SLOTS_PER_RECORD)
            .map(|minute| i16::try_from(minute * 2 - 1).unwrap())
            .collect::<Vec<_>>();
        let mask_off = (1..=MAX_MASK_SLOTS_PER_RECORD)
            .map(|minute| i16::try_from(minute * 2).unwrap())
            .collect::<Vec<_>>();
        let bytes = standard_str(
            &mask_on,
            &mask_off,
            &[i16::try_from(MAX_MASK_SLOTS_PER_RECORD * 2).unwrap()],
            1,
        );
        let decoded = decode_str(&bytes, options(Some("serial-123"), current(2027, 1, 1))).unwrap();
        assert_eq!(decoded.days[0].boundaries.len(), MAX_MASK_SLOTS_PER_RECORD);
        assert_eq!(decoded.diagnostics.ambiguous_days, 0);
        assert_eq!(decoded.diagnostics.mask_event_count_mismatch_days, 0);
    }

    #[test]
    fn dates_roll_over_and_resmed_repairs_pre_2000_edf_years() {
        let bytes = synthetic_str(
            &[
                SignalFixture::new("Mask On", 1, &[100, 100, 100]),
                SignalFixture::new("Mask Off", 1, &[200, 200, 200]),
                SignalFixture::new("Mask Events", 1, &[2, 2, 2]),
            ],
            3,
            "SRN=serial-123",
            "28.02.8812.00.00",
            "86400",
            "",
        );
        let decoded = decode_str(&bytes, options(Some("serial-123"), current(2089, 1, 1))).unwrap();
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
    fn requires_plain_daily_noon_records_without_trailing_bytes() {
        let base_signals = [
            SignalFixture::new("Mask On", 1, &[100]),
            SignalFixture::new("Mask Off", 1, &[200]),
            SignalFixture::new("Mask Events", 1, &[2]),
        ];
        let wrong_duration = synthetic_str(
            &base_signals,
            1,
            "SRN=serial-123",
            "01.01.2612.00.00",
            "3600",
            "",
        );
        assert_eq!(
            decode_str(
                &wrong_duration,
                options(Some("serial-123"), current(2027, 1, 1))
            ),
            Err(StrDecodeError::InvalidRecordDuration)
        );

        let wrong_time = synthetic_str(
            &base_signals,
            1,
            "SRN=serial-123",
            "01.01.2611.59.59",
            "86400",
            "",
        );
        assert_eq!(
            decode_str(
                &wrong_time,
                options(Some("serial-123"), current(2027, 1, 1))
            ),
            Err(StrDecodeError::InvalidRecordStart)
        );

        let mut trailing = standard_str(&[100], &[200], &[2], 1);
        trailing.extend_from_slice(b"secret trailing bytes");
        assert_eq!(
            decode_str(&trailing, options(Some("serial-123"), current(2027, 1, 1))),
            Err(StrDecodeError::TrailingData { bytes: 21 })
        );
    }

    #[test]
    fn rejects_invalid_current_time_and_over_limit_input_before_parsing() {
        let bytes = standard_str(&[100], &[200], &[2], 1);
        let invalid_now = DeviceLocalDateTime {
            year: 2026,
            month: 2,
            day: 30,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
        };
        assert_eq!(
            decode_str(&bytes, options(Some("serial-123"), invalid_now)),
            Err(StrDecodeError::InvalidCurrentDeviceTime)
        );

        let oversized = vec![0; RESMED_STR_MAX_FILE_BYTES + 1];
        assert_eq!(
            decode_str(&oversized, options(Some("serial-123"), current(2027, 1, 1))),
            Err(StrDecodeError::FileTooLarge {
                limit: RESMED_STR_MAX_FILE_BYTES,
                actual: RESMED_STR_MAX_FILE_BYTES + 1,
            })
        );
    }
}
