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

//! Bounded decoding of ResMed PLD sampled signals.
//!
//! This module ports the signal dispatch and normalization performed by
//! OSCAR's pinned `ResmedLoader::LoadPLD`/`ToTimeDelta` implementation. It does
//! not group files into sessions or choose session identity. The caller
//! supplies the header snapshot captured during indexing so decoding can fail
//! closed if indexed header fields drift between those two phases. Payload
//! identity remains the responsibility of the enclosing import boundary.
//!
//! Intentional hardening differences from the pinned implementation:
//!
//! - EDF calibration uses the complete affine transform (gain and offset).
//!   The pinned OSCAR parser initializes the offset to zero.
//! - raw digital `-1` values always delimit missing-data segments before
//!   calibration instead of being repeated from the preceding sample;
//! - samples outside the signal's declared digital range delimit segments,
//!   preserving OSCAR's physical-range rejection under the corrected affine
//!   calibration;
//! - parser, output-sample, output-segment, and input-byte allocations are
//!   explicitly bounded; and
//! - machine-identity failures never include either serial number.
//!
//! `opap-edf` intentionally enforces EDF's ASCII signal-header contract.
//! Non-ASCII legacy aliases remain in the channel registry for provenance, but
//! require a future bounded ResMed-specific field decoder before they can
//! occur in a decoded PLD file.

use super::{ResmedEdfHeaderSummary, ResmedSessionFileKind};
use crate::{
    domain::EdfSourceEncoding,
    importer::{ImportError, ImportErrorKind},
};
use opap_channels::{ChannelDefinition, ResmedFileKind, by_stable_key};
use opap_edf::{EdfFile, EdfHeader, Limits, ParseErrorKind, Parser, Signal};

/// Maximum bytes accepted from one uncompressed PLD payload.
pub(super) const RESMED_PLD_MAX_FILE_BYTES: usize = 256 * 1024 * 1024;

/// Maximum normalized PLD samples materialized from one payload.
pub(super) const RESMED_PLD_MAX_OUTPUT_SAMPLES: usize = 50_000_000;

/// Maximum missing-data-delimited series materialized from one payload.
pub(super) const RESMED_PLD_MAX_OUTPUT_SEGMENTS: usize = 100_000;

const RESMED_PLD_MAX_SIGNALS: usize = 64;
const RESMED_PLD_MAX_RECORDS: usize = 1_000_000;
const RESMED_PLD_MAX_SIGNAL_RECORDS: usize = 8_000_000;
const RESMED_PLD_MAX_TOTAL_SAMPLES: usize = 50_000_000;
const RESMED_PLD_MAX_ANNOTATION_BYTES: usize = 1024 * 1024;
const RESMED_PLD_MAX_ANNOTATION_RECORDS: usize = 100_000;
const RESMED_PLD_MAX_ANNOTATIONS: usize = 100_000;
const RESMED_PLD_MAX_ANNOTATION_TEXT_BYTES: usize = 1024 * 1024;

const PLD_DISPATCH_ORDER: [&str; 13] = [
    // Keep this in the order of the pinned LoadPLD if/else chain. In
    // particular, tidal volume must precede inspiratory time: "TidVol.2s"
    // also starts with the short "Ti" alias.
    "pap.series.snore",
    "pap.series.therapy_pressure",
    "pap.series.ipap",
    "pap.series.epap",
    "pap.series.minute_ventilation",
    "pap.series.respiratory_rate",
    "pap.series.tidal_volume",
    "pap.series.leak_rate",
    "pap.series.flow_limitation",
    "pap.series.mask_pressure",
    "pap.series.inspiratory_time",
    "pap.series.expiratory_time",
    "pap.series.target_minute_ventilation",
];

const INSPIRATORY_TIME_CHANNEL: &str = "pap.series.inspiratory_time";
const EXPIRATORY_TIME_CHANNEL: &str = "pap.series.expiratory_time";
const LEAK_RATE_CHANNEL: &str = "pap.series.leak_rate";
const TIDAL_VOLUME_CHANNEL: &str = "pap.series.tidal_volume";

#[derive(Debug, Clone, Copy)]
struct PldDecodeLimits {
    max_file_bytes: usize,
    max_signals: usize,
    max_records: usize,
    max_signal_records: usize,
    max_total_samples: usize,
    max_output_samples: usize,
    max_output_segments: usize,
}

impl PldDecodeLimits {
    const DEFAULT: Self = Self {
        max_file_bytes: RESMED_PLD_MAX_FILE_BYTES,
        max_signals: RESMED_PLD_MAX_SIGNALS,
        max_records: RESMED_PLD_MAX_RECORDS,
        max_signal_records: RESMED_PLD_MAX_SIGNAL_RECORDS,
        max_total_samples: RESMED_PLD_MAX_TOTAL_SAMPLES,
        max_output_samples: RESMED_PLD_MAX_OUTPUT_SAMPLES,
        max_output_segments: RESMED_PLD_MAX_OUTPUT_SEGMENTS,
    };

    const fn parser_limits(self) -> Limits {
        Limits {
            max_signals: self.max_signals,
            max_records: self.max_records,
            max_signal_records: self.max_signal_records,
            max_total_samples: self.max_total_samples,
            max_annotation_bytes: RESMED_PLD_MAX_ANNOTATION_BYTES,
            max_annotation_records: RESMED_PLD_MAX_ANNOTATION_RECORDS,
            max_annotations: RESMED_PLD_MAX_ANNOTATIONS,
            max_annotation_text_bytes: RESMED_PLD_MAX_ANNOTATION_TEXT_BYTES,
        }
    }
}

/// Caller-owned remaining PLD output capacity across an import.
///
/// The decoder charges this budget only after a complete payload succeeds.
/// This lets an optional, rejected PLD file be discarded without consuming the
/// capacity reserved for later files while still preventing aggregate output
/// from exceeding the enclosing import's limits.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PldDecodeBudget {
    remaining_samples: usize,
    remaining_segments: usize,
}

#[allow(dead_code)]
impl PldDecodeBudget {
    /// Create an aggregate budget supplied by the enclosing import.
    pub(super) const fn new(remaining_samples: usize, remaining_segments: usize) -> Self {
        Self {
            remaining_samples,
            remaining_segments,
        }
    }

    /// Remaining normalized sample capacity.
    pub(super) const fn remaining_samples(self) -> usize {
        self.remaining_samples
    }

    /// Remaining missing-data-delimited segment capacity.
    pub(super) const fn remaining_segments(self) -> usize {
        self.remaining_segments
    }
}

impl Default for PldDecodeBudget {
    fn default() -> Self {
        Self::new(
            RESMED_PLD_MAX_OUTPUT_SAMPLES,
            RESMED_PLD_MAX_OUTPUT_SEGMENTS,
        )
    }
}

/// A decoded PLD payload, still expressed relative to the EDF file start.
///
/// Session attachment supplies the normalized start time and opaque child keys
/// later, so PLD content cannot alter logical session identity.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DecodedPldFile {
    /// Number of complete EDF data records decoded.
    pub record_count: usize,
    /// Duration of one EDF data record.
    pub record_duration_seconds: f64,
    /// Supported sampled signals in source order.
    pub signals: Vec<DecodedPldSignal>,
    /// Non-fatal per-signal diagnostics containing no source identity text.
    pub warnings: Vec<PldDecodeWarning>,
}

/// One supported PLD channel and its missing-data-delimited segments.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DecodedPldSignal {
    /// Zero-based signal descriptor position in the EDF header.
    pub signal_index: usize,
    /// Stable OPAP channel key.
    pub channel_id: &'static str,
    /// Neutral display label from the channel registry.
    pub channel_label: &'static str,
    /// Canonical unit symbol after PLD normalization.
    pub unit: &'static str,
    /// Milliseconds between adjacent source samples.
    pub sample_interval_ms: f64,
    /// Original, unnormalized EDF calibration and cadence.
    pub source_encoding: EdfSourceEncoding,
    /// Original EDF physical-dimension field.
    pub source_physical_dimension: String,
    /// Factor applied after the full EDF affine calibration.
    pub normalization_scale: f64,
    /// OSCAR-compatible leading samples omitted from this channel.
    pub leading_samples_trimmed: usize,
    /// Contiguous valid segments. Digital `-1` and out-of-range values are
    /// never included.
    pub segments: Vec<DecodedPldSegment>,
}

/// One contiguous run of valid normalized PLD samples.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DecodedPldSegment {
    /// Index of the first sample relative to the complete source signal.
    pub start_sample_index: usize,
    /// Fully calibrated values in the channel's canonical unit.
    pub samples: Vec<f32>,
}

/// Stable, privacy-safe reason for a non-fatal PLD signal rejection.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PldDecodeWarningKind {
    /// The signal was not part of the supported pinned PLD set.
    UnknownSignal,
    /// A second Ti/Te signal was ignored like the pinned loader.
    DuplicateRespiratoryTime,
    /// The signal had no digital samples.
    NotDigital,
    /// The signal cadence could not be represented safely.
    InvalidCadence,
    /// The EDF calibration was invalid.
    InvalidCalibration,
    /// Calibration or canonical-unit normalization did not fit finite `f32`.
    NonFiniteNormalizedSample,
    /// OSCAR's leading-sample trim left fewer than two samples.
    InsufficientSamplesAfterTrim,
    /// Every post-trim value was missing or outside declared bounds.
    NoValidSamples,
    /// Legacy scaling was applied despite an unexpected source dimension.
    UnexpectedSourceUnit,
}

impl PldDecodeWarningKind {
    /// Stable code suitable for conversion to [`crate::domain::ImportWarning`].
    #[allow(dead_code)]
    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::UnknownSignal => "unknown_resmed_pld_signal",
            Self::DuplicateRespiratoryTime => "duplicate_resmed_pld_respiratory_time",
            Self::NotDigital => "invalid_resmed_pld_signal",
            Self::InvalidCadence => "invalid_resmed_pld_cadence",
            Self::InvalidCalibration => "invalid_resmed_pld_calibration",
            Self::NonFiniteNormalizedSample => "invalid_resmed_pld_normalized_sample",
            Self::InsufficientSamplesAfterTrim => "short_resmed_pld_signal",
            Self::NoValidSamples => "empty_resmed_pld_signal",
            Self::UnexpectedSourceUnit => "unexpected_resmed_pld_source_unit",
        }
    }

    /// Privacy-safe diagnostic text that never includes an EDF header value.
    #[allow(dead_code)]
    pub(super) const fn message(self) -> &'static str {
        match self {
            Self::UnknownSignal => "Unsupported PLD signal was ignored",
            Self::DuplicateRespiratoryTime => "Duplicate PLD respiratory-time signal was ignored",
            Self::NotDigital => "PLD signal did not contain digital samples",
            Self::InvalidCadence => "PLD signal cadence was invalid",
            Self::InvalidCalibration => "PLD signal calibration was invalid",
            Self::NonFiniteNormalizedSample => {
                "PLD signal normalization produced a non-finite sample"
            }
            Self::InsufficientSamplesAfterTrim => {
                "PLD signal was too short after its leading stabilization trim"
            }
            Self::NoValidSamples => "PLD signal had no valid post-trim samples",
            Self::UnexpectedSourceUnit => {
                "PLD legacy unit conversion was applied to an unexpected source dimension"
            }
        }
    }
}

/// One non-fatal PLD signal diagnostic.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PldDecodeWarning {
    /// Stable warning category.
    pub kind: PldDecodeWarningKind,
    /// Zero-based EDF signal position, avoiding untrusted label disclosure.
    pub signal_index: usize,
    /// Stable channel key when resolution succeeded.
    pub channel_id: Option<&'static str>,
}

/// Decode one complete, uncompressed ResMed PLD EDF payload.
///
/// `indexed_header` must be the summary captured for the same file during
/// candidate indexing. A mismatch in those indexed fields is fatal.
/// `machine_serial` is compared with every non-empty `SRN=` token in the
/// recording header, but neither value is copied into an error or warning.
/// `budget` is aggregate caller-owned capacity across all PLD files in one
/// import and is charged only after this payload succeeds.
///
/// # Errors
///
/// Returns a bounded [`ImportError`] for malformed data, resource exhaustion,
/// header drift, discontinuous EDF input, or absent/mismatched machine identity.
#[allow(dead_code)]
pub(super) fn decode_pld_edf(
    bytes: &[u8],
    file_kind: ResmedSessionFileKind,
    machine_serial: &str,
    indexed_header: &ResmedEdfHeaderSummary,
    budget: &mut PldDecodeBudget,
) -> Result<DecodedPldFile, ImportError> {
    decode_pld_edf_with_limits(
        bytes,
        file_kind,
        machine_serial,
        indexed_header,
        budget,
        PldDecodeLimits::DEFAULT,
    )
}

fn decode_pld_edf_with_limits(
    bytes: &[u8],
    file_kind: ResmedSessionFileKind,
    machine_serial: &str,
    indexed_header: &ResmedEdfHeaderSummary,
    budget: &mut PldDecodeBudget,
    limits: PldDecodeLimits,
) -> Result<DecodedPldFile, ImportError> {
    validate_limits(limits)?;
    if file_kind != ResmedSessionFileKind::Pld {
        return Err(pld_error(
            ImportErrorKind::InvalidConfiguration,
            "PLD decoder received a non-PLD session file",
        ));
    }
    if budget.remaining_samples == 0 || budget.remaining_segments == 0 {
        return Err(output_limit_error(
            "Aggregate PLD output capacity is exhausted",
        ));
    }
    if bytes.len() > limits.max_file_bytes {
        return Err(pld_error(
            ImportErrorKind::SizeLimitExceeded,
            "PLD payload exceeds the configured byte limit",
        ));
    }
    let expected_serial = machine_serial.trim();
    if expected_serial.is_empty()
        || expected_serial
            .bytes()
            .any(|byte| byte.is_ascii_whitespace())
    {
        return Err(pld_error(
            ImportErrorKind::InvalidConfiguration,
            "PLD decoding requires one non-empty token-safe machine serial",
        ));
    }

    let parser = Parser::new(limits.parser_limits());
    let header = parser.parse_header(bytes).map_err(sanitized_parse_error)?;
    validate_indexed_header(&header, indexed_header)?;
    validate_pld_header_contract(&header)?;
    validate_recording_serial(&header, expected_serial)?;

    let parsed = parser.parse(bytes).map_err(sanitized_parse_error)?;
    validate_complete_header(&parsed)?;

    let mut decoded = DecodedPldFile {
        record_count: parsed.record_count(),
        record_duration_seconds: parsed.header().record_duration_seconds,
        signals: Vec::new(),
        warnings: Vec::new(),
    };
    decoded
        .signals
        .try_reserve(parsed.signals().len())
        .map_err(|_| allocation_error("PLD decoded signal metadata"))?;
    let warning_capacity = parsed
        .signals()
        .len()
        .checked_mul(2)
        .ok_or_else(|| output_limit_error("PLD warning-capacity arithmetic overflowed"))?;
    decoded
        .warnings
        .try_reserve(warning_capacity)
        .map_err(|_| allocation_error("PLD warning metadata"))?;

    let mut output_samples = 0usize;
    let mut output_segments = 0usize;
    let mut found_inspiratory_time = false;
    let mut found_expiratory_time = false;

    for (signal_index, signal) in parsed.signals().iter().enumerate() {
        let label = signal.header.label.trim();
        let Some(channel) = resolve_pld_signal(label) else {
            if !is_silently_ignored_pld_label(label) {
                decoded.warnings.push(PldDecodeWarning {
                    kind: PldDecodeWarningKind::UnknownSignal,
                    signal_index,
                    channel_id: None,
                });
            }
            continue;
        };
        let channel_id = channel.key.as_str();
        if !source_dimension_is_expected(channel_id, &signal.header.physical_dimension) {
            decoded.warnings.push(PldDecodeWarning {
                kind: PldDecodeWarningKind::UnexpectedSourceUnit,
                signal_index,
                channel_id: Some(channel_id),
            });
        }

        let duplicate = match channel_id {
            INSPIRATORY_TIME_CHANNEL if found_inspiratory_time => true,
            INSPIRATORY_TIME_CHANNEL => {
                found_inspiratory_time = true;
                false
            }
            EXPIRATORY_TIME_CHANNEL if found_expiratory_time => true,
            EXPIRATORY_TIME_CHANNEL => {
                found_expiratory_time = true;
                false
            }
            _ => false,
        };
        if duplicate {
            decoded.warnings.push(PldDecodeWarning {
                kind: PldDecodeWarningKind::DuplicateRespiratoryTime,
                signal_index,
                channel_id: Some(channel_id),
            });
            continue;
        }

        let Some(raw_samples) = signal.digital_samples() else {
            decoded.warnings.push(PldDecodeWarning {
                kind: PldDecodeWarningKind::NotDigital,
                signal_index,
                channel_id: Some(channel_id),
            });
            continue;
        };
        let Some((sample_interval_ms, source_encoding)) =
            validated_signal_encoding(&parsed, signal)
        else {
            decoded.warnings.push(PldDecodeWarning {
                kind: PldDecodeWarningKind::InvalidCadence,
                signal_index,
                channel_id: Some(channel_id),
            });
            continue;
        };

        let leading_samples_trimmed = leading_trim_samples(channel_id);
        // Pinned ToTimeDelta only constructs a list when samples >
        // start_position + 1.
        if raw_samples.len() <= leading_samples_trimmed.saturating_add(1) {
            decoded.warnings.push(PldDecodeWarning {
                kind: PldDecodeWarningKind::InsufficientSamplesAfterTrim,
                signal_index,
                channel_id: Some(channel_id),
            });
            continue;
        }
        let raw_samples = &raw_samples[leading_samples_trimmed..];
        let normalization_scale = normalization_scale(channel_id);

        let (signal_samples, signal_segments) =
            match validate_signal_samples(signal, raw_samples, normalization_scale) {
                Ok(counts) => counts,
                Err(kind) => {
                    decoded.warnings.push(PldDecodeWarning {
                        kind,
                        signal_index,
                        channel_id: Some(channel_id),
                    });
                    continue;
                }
            };
        if signal_samples == 0 {
            decoded.warnings.push(PldDecodeWarning {
                kind: PldDecodeWarningKind::NoValidSamples,
                signal_index,
                channel_id: Some(channel_id),
            });
            continue;
        }
        output_samples = output_samples
            .checked_add(signal_samples)
            .ok_or_else(|| output_limit_error("PLD output-sample arithmetic overflowed"))?;
        if output_samples > limits.max_output_samples {
            return Err(output_limit_error(
                "PLD output exceeds the configured sample limit",
            ));
        }
        if output_samples > budget.remaining_samples {
            return Err(output_limit_error(
                "PLD output exceeds the remaining aggregate sample budget",
            ));
        }
        output_segments = output_segments
            .checked_add(signal_segments)
            .ok_or_else(|| output_limit_error("PLD output-segment arithmetic overflowed"))?;
        if output_segments > limits.max_output_segments {
            return Err(output_limit_error(
                "PLD output exceeds the configured segment limit",
            ));
        }
        if output_segments > budget.remaining_segments {
            return Err(output_limit_error(
                "PLD output exceeds the remaining aggregate segment budget",
            ));
        }

        let segments = materialize_segments(
            signal,
            raw_samples,
            leading_samples_trimmed,
            normalization_scale,
            signal_segments,
        )?;
        decoded.signals.push(DecodedPldSignal {
            signal_index,
            channel_id,
            channel_label: channel.label,
            unit: channel.unit.symbol(),
            sample_interval_ms,
            source_encoding,
            source_physical_dimension: signal.header.physical_dimension.clone(),
            normalization_scale,
            leading_samples_trimmed,
            segments,
        });
    }

    budget.remaining_samples = budget
        .remaining_samples
        .checked_sub(output_samples)
        .expect("PLD output was checked against aggregate sample capacity");
    budget.remaining_segments = budget
        .remaining_segments
        .checked_sub(output_segments)
        .expect("PLD output was checked against aggregate segment capacity");
    Ok(decoded)
}

fn validate_limits(limits: PldDecodeLimits) -> Result<(), ImportError> {
    if limits.max_file_bytes == 0
        || limits.max_signals == 0
        || limits.max_records == 0
        || limits.max_signal_records == 0
        || limits.max_total_samples == 0
        || limits.max_output_samples == 0
        || limits.max_output_segments == 0
    {
        return Err(pld_error(
            ImportErrorKind::InvalidConfiguration,
            "PLD decoder limits must all be non-zero",
        ));
    }
    Ok(())
}

fn validate_indexed_header(
    header: &EdfHeader,
    indexed: &ResmedEdfHeaderSummary,
) -> Result<(), ImportError> {
    let basic_fields_match = u64::try_from(header.header_bytes).ok() == Some(indexed.header_bytes)
        && u16::try_from(header.signals.len()).ok() == Some(indexed.signal_count)
        && header
            .declared_record_count
            .and_then(|count| u64::try_from(count).ok())
            == indexed.declared_record_count
        && header.record_duration_seconds.to_bits() == indexed.record_duration_seconds.to_bits();
    if !basic_fields_match {
        return Err(header_drift_error());
    }

    if let Some(start) = &indexed.start_time {
        let decoded = &header.start;
        // ResMed's pinned loader treats stored years 85..99 as 2085..2099,
        // unlike the generic EDF century convention. This matches the existing
        // candidate index/detail validation boundary.
        let decoded_year = if (85..=99).contains(&decoded.year_two_digits) {
            2_000 + u16::from(decoded.year_two_digits)
        } else {
            decoded.year
        };
        if decoded_year != start.year
            || decoded.month != start.month
            || decoded.day != start.day
            || decoded.hour != start.hour
            || decoded.minute != start.minute
            || decoded.second != start.second
            || start.millisecond != 0
        {
            return Err(header_drift_error());
        }
    }

    if let Some(indexed_duration) = indexed.estimated_duration_millis {
        let Some(records) = header.declared_record_count else {
            return Err(header_drift_error());
        };
        let Ok(records) = u32::try_from(records) else {
            return Err(header_drift_error());
        };
        let seconds = header.record_duration_seconds * f64::from(records);
        if !seconds.is_finite() || seconds < 0.0 || seconds > u64::MAX as f64 / 1_000.0 {
            return Err(header_drift_error());
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let decoded_duration = (seconds as u64).checked_mul(1_000);
        if decoded_duration != Some(indexed_duration) {
            return Err(header_drift_error());
        }
    }

    Ok(())
}

fn validate_pld_header_contract(header: &EdfHeader) -> Result<(), ImportError> {
    if header.is_discontinuous() {
        return Err(pld_error(
            ImportErrorKind::InvalidData,
            "Discontinuous EDF+D PLD payloads are not supported",
        ));
    }
    match header.declared_record_count {
        Some(0) => Err(pld_error(
            ImportErrorKind::InvalidData,
            "PLD payload declares no complete data records",
        )),
        Some(_) if header.record_duration_seconds > 0.0 => Ok(()),
        Some(_) => Err(pld_error(
            ImportErrorKind::InvalidData,
            "PLD payload declares a non-positive record duration",
        )),
        None => Err(pld_error(
            ImportErrorKind::InvalidData,
            "PLD payload must declare a known record count",
        )),
    }
}

fn validate_complete_header(parsed: &EdfFile) -> Result<(), ImportError> {
    if parsed.trailing_data_bytes() != 0 {
        return Err(pld_error(
            ImportErrorKind::InvalidData,
            "PLD payload contains bytes beyond its declared complete records",
        ));
    }
    for signal in parsed.signals() {
        let expected = parsed
            .record_count()
            .checked_mul(signal.header.samples_per_record)
            .ok_or_else(|| {
                pld_error(
                    ImportErrorKind::SizeLimitExceeded,
                    "PLD signal sample-count arithmetic overflowed",
                )
            })?;
        let actual = signal.digital_samples().map_or_else(
            || {
                signal
                    .annotation_records()
                    .map_or(0, |records| records.len())
            },
            <[i16]>::len,
        );
        if signal.digital_samples().is_some() && actual != expected {
            return Err(pld_error(
                ImportErrorKind::InvalidData,
                "PLD signal samples are inconsistent with the decoded header",
            ));
        }
    }
    Ok(())
}

fn validate_recording_serial(header: &EdfHeader, expected_serial: &str) -> Result<(), ImportError> {
    let mut found = false;
    for token in header.recording_id.split_ascii_whitespace() {
        let Some(serial) = token.strip_prefix("SRN=") else {
            continue;
        };
        if serial.is_empty() || serial != expected_serial {
            return Err(pld_error(
                ImportErrorKind::InvalidData,
                "PLD machine identity did not match the selected card",
            ));
        }
        found = true;
    }
    if !found {
        return Err(pld_error(
            ImportErrorKind::InvalidData,
            "PLD recording header has no verifiable machine identity",
        ));
    }
    Ok(())
}

fn resolve_pld_signal(label: &str) -> Option<&'static ChannelDefinition> {
    for channel_id in PLD_DISPATCH_ORDER {
        let channel = by_stable_key(channel_id)
            .expect("the pinned PLD dispatch table contains only registered channels");
        if channel.resmed_signals.iter().any(|descriptor| {
            descriptor.file == ResmedFileKind::Pld
                && descriptor
                    .aliases
                    .iter()
                    .any(|alias| starts_with_case_insensitive(label, alias))
        }) {
            return Some(channel);
        }
    }
    None
}

fn starts_with_case_insensitive(value: &str, prefix: &str) -> bool {
    let mut value_lowercase = value.chars().flat_map(char::to_lowercase);
    prefix
        .chars()
        .flat_map(char::to_lowercase)
        .all(|expected| value_lowercase.next() == Some(expected))
}

fn is_silently_ignored_pld_label(label: &str) -> bool {
    label.is_empty()
        || label == "Crc16"
        || matches!(
            label,
            "Va" | "AlvMinVent.2s" | "CLRatio.2s" | "TRRatio.2s"
        )
        // OSCAR recognizes I:E but its persistence call is commented out at
        // the pinned revision.
        || starts_with_case_insensitive(label, "I:E")
        || starts_with_case_insensitive(label, "IERatio.2s")
}

fn leading_trim_samples(channel_id: &str) -> usize {
    match channel_id {
        "pap.series.therapy_pressure" | "pap.series.ipap" | "pap.series.epap" => 5,
        "pap.series.minute_ventilation"
        | "pap.series.respiratory_rate"
        | "pap.series.tidal_volume"
        | "pap.series.inspiratory_time"
        | "pap.series.expiratory_time" => 10,
        _ => 0,
    }
}

fn normalization_scale(channel_id: &str) -> f64 {
    match channel_id {
        LEAK_RATE_CHANNEL => 60.0,
        TIDAL_VOLUME_CHANNEL => 1_000.0,
        _ => 1.0,
    }
}

fn source_dimension_is_expected(channel_id: &str, dimension: &str) -> bool {
    let dimension = dimension.trim();
    match channel_id {
        // Pinned OSCAR converts ResMed's per-second leak source to L/min.
        LEAK_RATE_CHANNEL => {
            dimension.eq_ignore_ascii_case("L/s")
                || dimension.eq_ignore_ascii_case("L/sec")
                || dimension.eq_ignore_ascii_case("Lps")
        }
        // Pinned OSCAR converts tidal volume from litres to millilitres.
        TIDAL_VOLUME_CHANNEL => {
            dimension.eq_ignore_ascii_case("L")
                || dimension.eq_ignore_ascii_case("liter")
                || dimension.eq_ignore_ascii_case("litre")
        }
        _ => true,
    }
}

fn validated_signal_encoding(
    parsed: &EdfFile,
    signal: &Signal,
) -> Option<(f64, EdfSourceEncoding)> {
    let samples_per_record = u32::try_from(signal.header.samples_per_record).ok()?;
    if samples_per_record == 0 {
        return None;
    }
    let sample_interval_ms =
        parsed.header().record_duration_seconds * 1_000.0 / f64::from(samples_per_record);
    if !sample_interval_ms.is_finite() || sample_interval_ms <= 0.0 {
        return None;
    }
    Some((
        sample_interval_ms,
        EdfSourceEncoding {
            digital_minimum: signal.header.digital_minimum,
            digital_maximum: signal.header.digital_maximum,
            physical_minimum: signal.header.physical_minimum,
            physical_maximum: signal.header.physical_maximum,
            samples_per_record,
            record_duration_seconds: parsed.header().record_duration_seconds,
        },
    ))
}

fn validate_signal_samples(
    signal: &Signal,
    raw_samples: &[i16],
    scale: f64,
) -> Result<(usize, usize), PldDecodeWarningKind> {
    signal
        .header
        .gain()
        .and_then(|_| signal.header.offset())
        .map_err(|_| PldDecodeWarningKind::InvalidCalibration)?;

    let mut sample_count = 0usize;
    let mut segment_count = 0usize;
    let mut in_segment = false;
    for &raw in raw_samples {
        if sample_is_missing_or_outside_declared_range(signal, raw) {
            in_segment = false;
            continue;
        }
        if !in_segment {
            segment_count = segment_count
                .checked_add(1)
                .ok_or(PldDecodeWarningKind::NonFiniteNormalizedSample)?;
            in_segment = true;
        }
        normalized_sample(signal, raw, scale)?;
        sample_count = sample_count
            .checked_add(1)
            .ok_or(PldDecodeWarningKind::NonFiniteNormalizedSample)?;
    }
    Ok((sample_count, segment_count))
}

fn sample_is_missing_or_outside_declared_range(signal: &Signal, raw: i16) -> bool {
    if raw == -1 {
        return true;
    }
    let raw = i32::from(raw);
    let minimum = signal
        .header
        .digital_minimum
        .min(signal.header.digital_maximum);
    let maximum = signal
        .header
        .digital_minimum
        .max(signal.header.digital_maximum);
    !(minimum..=maximum).contains(&raw)
}

fn normalized_sample(signal: &Signal, raw: i16, scale: f64) -> Result<f32, PldDecodeWarningKind> {
    let physical = signal
        .header
        .physical_value(raw)
        .map_err(|_| PldDecodeWarningKind::InvalidCalibration)?;
    let normalized = physical * scale;
    #[allow(clippy::cast_possible_truncation)]
    let normalized = normalized as f32;
    normalized
        .is_finite()
        .then_some(normalized)
        .ok_or(PldDecodeWarningKind::NonFiniteNormalizedSample)
}

fn materialize_segments(
    signal: &Signal,
    raw_samples: &[i16],
    leading_samples_trimmed: usize,
    scale: f64,
    segment_count: usize,
) -> Result<Vec<DecodedPldSegment>, ImportError> {
    let mut segments = Vec::new();
    segments
        .try_reserve(segment_count)
        .map_err(|_| allocation_error("PLD output segments"))?;
    let mut current_start = None;
    let mut current_samples = Vec::new();

    for (relative_index, &raw) in raw_samples.iter().enumerate() {
        if sample_is_missing_or_outside_declared_range(signal, raw) {
            push_segment(&mut segments, &mut current_start, &mut current_samples)?;
            continue;
        }
        if current_start.is_none() {
            current_start = Some(
                leading_samples_trimmed
                    .checked_add(relative_index)
                    .ok_or_else(|| output_limit_error("PLD segment index overflowed"))?,
            );
        }
        current_samples
            .try_reserve(1)
            .map_err(|_| allocation_error("PLD output samples"))?;
        current_samples.push(
            normalized_sample(signal, raw, scale)
                .expect("PLD samples were validated before materialization"),
        );
    }
    push_segment(&mut segments, &mut current_start, &mut current_samples)?;
    Ok(segments)
}

fn push_segment(
    segments: &mut Vec<DecodedPldSegment>,
    start: &mut Option<usize>,
    samples: &mut Vec<f32>,
) -> Result<(), ImportError> {
    let Some(start_sample_index) = start.take() else {
        return Ok(());
    };
    if samples.is_empty() {
        return Err(pld_error(
            ImportErrorKind::InvalidData,
            "PLD segment state was internally inconsistent",
        ));
    }
    segments.push(DecodedPldSegment {
        start_sample_index,
        samples: std::mem::take(samples),
    });
    Ok(())
}

fn sanitized_parse_error(error: opap_edf::ParseError) -> ImportError {
    let kind = match error.kind {
        ParseErrorKind::LimitExceeded { .. }
        | ParseErrorKind::AllocationFailed { .. }
        | ParseErrorKind::ArithmeticOverflow { .. } => ImportErrorKind::SizeLimitExceeded,
        _ => ImportErrorKind::InvalidData,
    };
    pld_error(kind, "PLD payload is not a valid bounded EDF stream")
}

fn pld_error(kind: ImportErrorKind, message: &'static str) -> ImportError {
    ImportError::new(kind, message)
}

fn header_drift_error() -> ImportError {
    pld_error(
        ImportErrorKind::InvalidData,
        "Complete PLD header no longer matches its indexed summary",
    )
}

fn output_limit_error(message: &'static str) -> ImportError {
    pld_error(ImportErrorKind::SizeLimitExceeded, message)
}

fn allocation_error(resource: &'static str) -> ImportError {
    ImportError::new(
        ImportErrorKind::SizeLimitExceeded,
        format!("Could not allocate bounded {resource}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resmed::ResmedDeviceLocalTime;
    use opap_channels::ResmedFileKind;

    const SERIAL: &str = "pld-test-serial";

    #[derive(Clone)]
    struct SignalFixture {
        label: String,
        samples_per_record: usize,
        samples: Vec<i16>,
        physical_dimension: String,
        physical_minimum: f64,
        physical_maximum: f64,
        digital_minimum: i32,
        digital_maximum: i32,
    }

    impl SignalFixture {
        fn new(label: impl Into<String>, samples_per_record: usize, samples: Vec<i16>) -> Self {
            let label = label.into();
            let physical_dimension = resolve_pld_signal(&label).map_or_else(
                || "unit".to_owned(),
                |channel| match channel.key.as_str() {
                    LEAK_RATE_CHANNEL => "L/s".to_owned(),
                    TIDAL_VOLUME_CHANNEL => "L".to_owned(),
                    _ => "unit".to_owned(),
                },
            );
            Self {
                label,
                samples_per_record,
                samples,
                physical_dimension,
                physical_minimum: 0.0,
                physical_maximum: 100.0,
                digital_minimum: 0,
                digital_maximum: 100,
            }
        }

        fn calibration(
            mut self,
            physical_minimum: f64,
            physical_maximum: f64,
            digital_minimum: i32,
            digital_maximum: i32,
        ) -> Self {
            self.physical_minimum = physical_minimum;
            self.physical_maximum = physical_maximum;
            self.digital_minimum = digital_minimum;
            self.digital_maximum = digital_maximum;
            self
        }

        fn dimension(mut self, physical_dimension: impl Into<String>) -> Self {
            self.physical_dimension = physical_dimension.into();
            self
        }
    }

    #[test]
    fn every_registered_pld_alias_resolves_in_pinned_order_with_exact_scale_and_trim() {
        for channel_id in PLD_DISPATCH_ORDER {
            let definition = by_stable_key(channel_id).expect("registered PLD channel");
            let aliases = definition
                .resmed_signals
                .iter()
                .find(|descriptor| descriptor.file == ResmedFileKind::Pld)
                .expect("PLD descriptor")
                .aliases;
            for alias in aliases {
                assert_eq!(
                    resolve_pld_signal(alias).map(|channel| channel.key.as_str()),
                    Some(channel_id),
                    "{channel_id}/{alias:?}"
                );
                // opap-edf intentionally enforces EDF's ASCII header contract.
                // Preserve every registry alias in resolver coverage while
                // exercising every representable label end to end.
                if !alias.is_ascii() {
                    continue;
                }
                let fixture =
                    SignalFixture::new(*alias, 12, vec![50; 12]).calibration(1.0, 3.0, 0, 100);
                let bytes = synthetic_pld(&[fixture], 1, 2.0, &recording_id(SERIAL));
                let decoded = decode(&bytes, SERIAL).expect("supported alias decodes");
                assert!(decoded.warnings.is_empty(), "{channel_id}/{alias:?}");
                assert_eq!(decoded.signals.len(), 1, "{channel_id}/{alias:?}");
                let signal = &decoded.signals[0];
                let trim = leading_trim_samples(channel_id);
                assert_eq!(signal.channel_id, channel_id, "{alias:?}");
                assert_eq!(signal.leading_samples_trimmed, trim, "{alias:?}");
                assert_eq!(signal.normalization_scale, normalization_scale(channel_id));
                assert_eq!(signal.segments.len(), 1, "{alias:?}");
                assert_eq!(signal.segments[0].start_sample_index, trim, "{alias:?}");
                assert_eq!(
                    signal.segments[0].samples,
                    vec![(2.0 * normalization_scale(channel_id)) as f32; 12 - trim],
                    "{channel_id}/{alias:?}"
                );
            }
        }
    }

    #[test]
    fn ordered_resolver_prefers_tidal_volume_over_short_ti_prefix() {
        let resolved = resolve_pld_signal("tIdVoL.2S x").expect("known PLD label");
        assert_eq!(resolved.key.as_str(), TIDAL_VOLUME_CHANNEL);

        let bytes = synthetic_pld(
            &[SignalFixture::new("tIdVoL.2S x", 12, vec![2; 12])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded = decode(&bytes, SERIAL).expect("decode ambiguous prefix");
        assert_eq!(decoded.signals[0].channel_id, TIDAL_VOLUME_CHANNEL);
        assert_eq!(decoded.signals[0].normalization_scale, 1_000.0);
    }

    #[test]
    fn leak_and_tidal_volume_scale_while_minute_ventilation_and_rate_do_not() {
        for (label, expected_channel, expected_scale) in [
            ("Leak.2s", LEAK_RATE_CHANNEL, 60.0),
            ("TidVol.2s", TIDAL_VOLUME_CHANNEL, 1_000.0),
            ("MinVent.2s", "pap.series.minute_ventilation", 1.0),
            ("RespRate.2s", "pap.series.respiratory_rate", 1.0),
        ] {
            let bytes = synthetic_pld(
                &[SignalFixture::new(label, 12, vec![2; 12])],
                1,
                2.0,
                &recording_id(SERIAL),
            );
            let decoded = decode(&bytes, SERIAL).expect("scaled signal");
            let signal = &decoded.signals[0];
            assert_eq!(signal.channel_id, expected_channel);
            assert_eq!(signal.normalization_scale, expected_scale);
            assert!(
                signal.segments[0]
                    .samples
                    .iter()
                    .all(|sample| *sample == (2.0 * expected_scale) as f32)
            );
        }
    }

    #[test]
    fn source_dimension_is_preserved_and_unexpected_legacy_scaling_is_diagnosed() {
        let expected = synthetic_pld(
            &[SignalFixture::new("Leak.2s", 2, vec![1, 2]).dimension("L/s")],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded = decode(&expected, SERIAL).expect("expected leak dimension");
        assert!(decoded.warnings.is_empty());
        assert_eq!(decoded.signals[0].source_physical_dimension, "L/s");
        assert_eq!(decoded.signals[0].segments[0].samples, vec![60.0, 120.0]);

        let already_normalized = synthetic_pld(
            &[SignalFixture::new("Leak.2s", 2, vec![1, 2]).dimension("L/min")],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded =
            decode(&already_normalized, SERIAL).expect("legacy conversion remains non-fatal");
        assert_eq!(decoded.signals[0].source_physical_dimension, "L/min");
        // This deliberately preserves the pinned loader's label-selected x60
        // transform, but the diagnostic prevents consumers from presenting it
        // as unit-verified data.
        assert_eq!(decoded.signals[0].segments[0].samples, vec![60.0, 120.0]);
        assert_eq!(
            decoded.warnings,
            vec![PldDecodeWarning {
                kind: PldDecodeWarningKind::UnexpectedSourceUnit,
                signal_index: 0,
                channel_id: Some(LEAK_RATE_CHANNEL),
            }]
        );
    }

    #[test]
    fn calibration_uses_affine_offset_not_only_gain() {
        let fixture = SignalFixture::new("MaskPress.2s", 3, vec![-100, 0, 100])
            .calibration(10.0, 20.0, -100, 100);
        let bytes = synthetic_pld(&[fixture], 1, 3.0, &recording_id(SERIAL));
        let decoded = decode(&bytes, SERIAL).expect("affine calibration");
        assert_eq!(
            decoded.signals[0].segments[0].samples,
            vec![10.0, 15.0, 20.0]
        );
        assert_eq!(decoded.signals[0].source_encoding.physical_minimum, 10.0);
    }

    #[test]
    fn raw_minus_one_splits_segments_before_calibration_and_preserves_source_offsets() {
        let fixture = SignalFixture::new("Leak.2s", 8, vec![-1, 1, 2, -1, 3, 4, -1, -1]);
        let bytes = synthetic_pld(&[fixture], 1, 4.0, &recording_id(SERIAL));
        let decoded = decode(&bytes, SERIAL).expect("gapped PLD");
        let segments = &decoded.signals[0].segments;
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start_sample_index, 1);
        assert_eq!(segments[0].samples, vec![60.0, 120.0]);
        assert_eq!(segments[1].start_sample_index, 4);
        assert_eq!(segments[1].samples, vec![180.0, 240.0]);
    }

    #[test]
    fn values_outside_declared_digital_range_split_segments() {
        let fixture =
            SignalFixture::new("Snore.2s", 5, vec![1, 2, 20, 3, 4]).calibration(0.0, 1.0, 0, 10);
        let bytes = synthetic_pld(&[fixture], 1, 2.0, &recording_id(SERIAL));
        let decoded = decode(&bytes, SERIAL).expect("bounded digital range");
        let segments = &decoded.signals[0].segments;
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start_sample_index, 0);
        assert_eq!(segments[0].samples, vec![0.1, 0.2]);
        assert_eq!(segments[1].start_sample_index, 3);
        assert_eq!(segments[1].samples, vec![0.3, 0.4]);
    }

    #[test]
    fn an_all_missing_supported_signal_is_diagnosed_without_an_empty_series() {
        let bytes = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 3, vec![-1; 3])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded = decode(&bytes, SERIAL).expect("all-missing PLD is non-fatal");
        assert!(decoded.signals.is_empty());
        assert_eq!(
            decoded.warnings,
            vec![PldDecodeWarning {
                kind: PldDecodeWarningKind::NoValidSamples,
                signal_index: 0,
                channel_id: Some("pap.series.snore"),
            }]
        );
    }

    #[test]
    fn stabilization_trim_precedes_gap_segmentation() {
        let fixture = SignalFixture::new(
            "Press.2s",
            11,
            vec![99, 99, 99, 99, 99, -1, 10, 20, -1, 30, 40],
        );
        let bytes = synthetic_pld(&[fixture], 1, 2.0, &recording_id(SERIAL));
        let decoded = decode(&bytes, SERIAL).expect("trimmed pressure");
        let signal = &decoded.signals[0];
        assert_eq!(signal.leading_samples_trimmed, 5);
        assert_eq!(signal.segments.len(), 2);
        assert_eq!(signal.segments[0].start_sample_index, 6);
        assert_eq!(signal.segments[0].samples, vec![10.0, 20.0]);
        assert_eq!(signal.segments[1].start_sample_index, 9);
        assert_eq!(signal.segments[1].samples, vec![30.0, 40.0]);
    }

    #[test]
    fn multi_record_samples_remain_chronological_with_record_cadence() {
        let bytes = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2, 3, 4])],
            2,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded = decode(&bytes, SERIAL).expect("multi-record PLD");
        assert_eq!(decoded.record_count, 2);
        assert_eq!(decoded.record_duration_seconds, 2.0);
        assert_eq!(decoded.signals[0].sample_interval_ms, 1_000.0);
        assert_eq!(
            decoded.signals[0].segments[0].samples,
            vec![1.0, 2.0, 3.0, 4.0]
        );
    }

    #[test]
    fn only_first_ti_and_te_are_persisted_like_pinned_loader() {
        let values = vec![10; 12];
        let bytes = synthetic_pld(
            &[
                SignalFixture::new("B5ITime.2s", 12, values.clone()),
                SignalFixture::new("Ti.2s second", 12, values.clone()),
                SignalFixture::new("B5ETime.2s", 12, values.clone()),
                SignalFixture::new("Te.2s second", 12, values),
            ],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded = decode(&bytes, SERIAL).expect("duplicate respiratory time");
        assert_eq!(decoded.signals.len(), 2);
        assert_eq!(
            decoded
                .warnings
                .iter()
                .filter(|warning| {
                    warning.kind == PldDecodeWarningKind::DuplicateRespiratoryTime
                })
                .count(),
            2
        );
    }

    #[test]
    fn pinned_nonpersisted_and_checksum_signals_are_silent_but_unknown_is_structured() {
        let bytes = synthetic_pld(
            &[
                SignalFixture::new("Crc16", 2, vec![0, 0]),
                SignalFixture::new("IERatio.2s", 2, vec![1, 1]),
                SignalFixture::new("Va", 2, vec![1, 1]),
                SignalFixture::new("private-looking", 2, vec![1, 1]),
            ],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded = decode(&bytes, SERIAL).expect("ignored PLD signals");
        assert!(decoded.signals.is_empty());
        assert_eq!(decoded.warnings.len(), 1);
        let warning = decoded.warnings[0];
        assert_eq!(warning.kind, PldDecodeWarningKind::UnknownSignal);
        assert_eq!(warning.signal_index, 3);
        assert_eq!(warning.channel_id, None);
        assert!(!warning.kind.message().contains("private-looking"));
    }

    #[test]
    fn invalid_calibration_and_non_finite_f32_output_skip_only_the_signal() {
        let bytes = synthetic_pld(
            &[
                SignalFixture::new("Snore.2s", 2, vec![0, 1]).calibration(0.0, 1.0, 0, 0),
                SignalFixture::new("Leak.2s", 2, vec![0, 100]).calibration(0.0, 1e307, 0, 100),
                SignalFixture::new("MaskPress.2s", 2, vec![0, 100]),
            ],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded = decode(&bytes, SERIAL).expect("per-signal rejection");
        assert_eq!(decoded.signals.len(), 1);
        assert_eq!(decoded.signals[0].channel_id, "pap.series.mask_pressure");
        assert!(decoded.warnings.iter().any(|warning| {
            warning.kind == PldDecodeWarningKind::InvalidCalibration && warning.signal_index == 0
        }));
        assert!(decoded.warnings.iter().any(|warning| {
            warning.kind == PldDecodeWarningKind::NonFiniteNormalizedSample
                && warning.signal_index == 1
        }));
    }

    #[test]
    fn zero_sample_cadence_is_a_non_fatal_signal_error() {
        let bytes = synthetic_pld(
            &[
                SignalFixture::new("Snore.2s", 0, Vec::new()),
                SignalFixture::new("MaskPress.2s", 2, vec![1, 2]),
            ],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let decoded = decode(&bytes, SERIAL).expect("one valid PLD signal survives");
        assert_eq!(decoded.signals.len(), 1);
        assert_eq!(decoded.signals[0].channel_id, "pap.series.mask_pressure");
        assert_eq!(
            decoded.warnings,
            vec![PldDecodeWarning {
                kind: PldDecodeWarningKind::InvalidCadence,
                signal_index: 0,
                channel_id: Some("pap.series.snore"),
            }]
        );
    }

    #[test]
    fn exact_oscar_trim_boundary_requires_two_remaining_samples() {
        for (label, sample_count, channel) in [
            ("Press.2s", 6, "pap.series.therapy_pressure"),
            ("MinVent.2s", 11, "pap.series.minute_ventilation"),
        ] {
            let bytes = synthetic_pld(
                &[SignalFixture::new(
                    label,
                    sample_count,
                    vec![1; sample_count],
                )],
                1,
                2.0,
                &recording_id(SERIAL),
            );
            let decoded = decode(&bytes, SERIAL).expect("short supported PLD");
            assert!(decoded.signals.is_empty());
            assert_eq!(
                decoded.warnings,
                vec![PldDecodeWarning {
                    kind: PldDecodeWarningKind::InsufficientSamplesAfterTrim,
                    signal_index: 0,
                    channel_id: Some(channel),
                }]
            );
        }
    }

    #[test]
    fn indexed_header_drift_is_rejected() {
        let bytes = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let indexed = indexed_header(&bytes);
        let mut mutations = Vec::new();

        let mut changed = indexed.clone();
        changed.header_bytes += 1;
        mutations.push(changed);
        let mut changed = indexed.clone();
        changed.signal_count += 1;
        mutations.push(changed);
        let mut changed = indexed.clone();
        changed.declared_record_count = Some(2);
        mutations.push(changed);
        let mut changed = indexed.clone();
        changed.record_duration_seconds = 3.0;
        mutations.push(changed);
        let mut changed = indexed.clone();
        changed.start_time.as_mut().expect("indexed start").second += 1;
        mutations.push(changed);
        let mut changed = indexed.clone();
        changed
            .start_time
            .as_mut()
            .expect("indexed start")
            .millisecond = 1;
        mutations.push(changed);
        let mut changed = indexed;
        changed.estimated_duration_millis = Some(3_000);
        mutations.push(changed);

        for changed in mutations {
            let error =
                decode_with_index(&bytes, SERIAL, &changed).expect_err("header drift rejected");
            assert_eq!(error.kind, ImportErrorKind::InvalidData);
            assert_eq!(
                error.message,
                "Complete PLD header no longer matches its indexed summary"
            );
        }
    }

    #[test]
    fn indexed_summary_is_a_header_guard_not_a_payload_fingerprint() {
        let original = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let indexed = indexed_header(&original);
        let mut changed_payload = original;
        let data_offset = usize::try_from(indexed.header_bytes).expect("header offset fits");
        changed_payload[data_offset..data_offset + 2].copy_from_slice(&9i16.to_le_bytes());

        let decoded = decode_with_index(&changed_payload, SERIAL, &indexed)
            .expect("unchanged indexed header remains valid");
        assert_eq!(decoded.signals[0].segments[0].samples, vec![9.0, 2.0]);
    }

    #[test]
    fn closed_pld_requires_a_known_positive_record_count() {
        let mut unknown_count = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        overwrite_field(&mut unknown_count, 236, 8, "-1");
        let error = decode(&unknown_count, SERIAL).expect_err("unknown record count rejected");
        assert_eq!(error.kind, ImportErrorKind::InvalidData);
        assert!(error.message.contains("known record count"));

        let no_records = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, Vec::new())],
            0,
            2.0,
            &recording_id(SERIAL),
        );
        let error = decode(&no_records, SERIAL).expect_err("zero record count rejected");
        assert_eq!(error.kind, ImportErrorKind::InvalidData);
        assert!(error.message.contains("no complete data records"));
    }

    #[test]
    fn serial_is_required_and_identity_errors_never_disclose_values() {
        let fixture = SignalFixture::new("Snore.2s", 2, vec![1, 2]);
        let missing = synthetic_pld(std::slice::from_ref(&fixture), 1, 2.0, "ResMed PLD");
        let mismatch = synthetic_pld(
            std::slice::from_ref(&fixture),
            1,
            2.0,
            &recording_id("other-private-serial"),
        );
        let conflicting = synthetic_pld(
            &[fixture],
            1,
            2.0,
            "ResMed SRN=pld-test-serial SRN=other-private-serial",
        );

        for bytes in [missing, mismatch, conflicting] {
            let error = decode(&bytes, SERIAL).expect_err("identity rejection");
            assert_eq!(error.kind, ImportErrorKind::InvalidData);
            assert!(!error.message.contains(SERIAL));
            assert!(!error.message.contains("other-private-serial"));
            assert!(error.relative_path.is_none());
        }
    }

    #[test]
    fn malformed_and_trailing_payloads_are_rejected_without_echoing_header_text() {
        let malformed = b"patient-secret".to_vec();
        let valid = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let error = decode_with_index(&malformed, SERIAL, &indexed_header(&valid))
            .expect_err("malformed EDF");
        assert_eq!(error.kind, ImportErrorKind::InvalidData);
        assert_eq!(
            error.message,
            "PLD payload is not a valid bounded EDF stream"
        );
        assert!(!error.message.contains("patient-secret"));

        let mut trailing = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let indexed = indexed_header(&trailing);
        trailing.extend_from_slice(&[0, 0]);
        let error =
            decode_with_index(&trailing, SERIAL, &indexed).expect_err("trailing payload rejected");
        assert_eq!(error.kind, ImportErrorKind::InvalidData);
        assert!(error.message.contains("beyond its declared"));
    }

    #[test]
    fn parser_and_output_resource_limits_fail_before_unbounded_materialization() {
        let two_signals = synthetic_pld(
            &[
                SignalFixture::new("Snore.2s", 4, vec![1, 2, 3, 4]),
                SignalFixture::new("Leak.2s", 4, vec![1, 2, 3, 4]),
            ],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let indexed = indexed_header(&two_signals);

        let error = decode_pld_edf_with_limits(
            &two_signals,
            ResmedSessionFileKind::Pld,
            SERIAL,
            &indexed,
            &mut PldDecodeBudget::default(),
            PldDecodeLimits {
                max_signals: 1,
                ..PldDecodeLimits::DEFAULT
            },
        )
        .expect_err("signal parser limit");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);

        let error = decode_pld_edf_with_limits(
            &two_signals,
            ResmedSessionFileKind::Pld,
            SERIAL,
            &indexed,
            &mut PldDecodeBudget::default(),
            PldDecodeLimits {
                max_total_samples: 7,
                ..PldDecodeLimits::DEFAULT
            },
        )
        .expect_err("parser sample limit");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);

        let error = decode_pld_edf_with_limits(
            &two_signals,
            ResmedSessionFileKind::Pld,
            SERIAL,
            &indexed,
            &mut PldDecodeBudget::default(),
            PldDecodeLimits {
                max_output_samples: 7,
                ..PldDecodeLimits::DEFAULT
            },
        )
        .expect_err("normalized output sample limit");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);

        let gapped = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 5, vec![1, -1, 2, -1, 3])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let error = decode_pld_edf_with_limits(
            &gapped,
            ResmedSessionFileKind::Pld,
            SERIAL,
            &indexed_header(&gapped),
            &mut PldDecodeBudget::default(),
            PldDecodeLimits {
                max_output_segments: 2,
                ..PldDecodeLimits::DEFAULT
            },
        )
        .expect_err("normalized segment limit");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);

        let error = decode_pld_edf_with_limits(
            &two_signals,
            ResmedSessionFileKind::Pld,
            SERIAL,
            &indexed,
            &mut PldDecodeBudget::default(),
            PldDecodeLimits {
                max_file_bytes: two_signals.len() - 1,
                ..PldDecodeLimits::DEFAULT
            },
        )
        .expect_err("input byte limit");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);
    }

    #[test]
    fn caller_budget_is_charged_only_after_success_and_bounds_multiple_files() {
        let bytes = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let indexed = indexed_header(&bytes);
        let mut budget = PldDecodeBudget::new(3, 2);
        let decoded = decode_pld_edf(
            &bytes,
            ResmedSessionFileKind::Pld,
            SERIAL,
            &indexed,
            &mut budget,
        )
        .expect("first file fits aggregate budget");
        assert_eq!(decoded.signals[0].segments[0].samples, vec![1.0, 2.0]);
        assert_eq!(budget.remaining_samples(), 1);
        assert_eq!(budget.remaining_segments(), 1);

        let error = decode_pld_edf(
            &bytes,
            ResmedSessionFileKind::Pld,
            SERIAL,
            &indexed,
            &mut budget,
        )
        .expect_err("second file exceeds remaining aggregate samples");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);
        assert_eq!(budget.remaining_samples(), 1);
        assert_eq!(budget.remaining_segments(), 1);
    }

    #[test]
    fn non_pld_kind_is_rejected_before_parsing_or_charging_budget() {
        let malformed = b"not-an-edf";
        let valid = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let indexed = indexed_header(&valid);
        let mut budget = PldDecodeBudget::new(2, 1);
        let error = decode_pld_edf(
            malformed,
            ResmedSessionFileKind::Brp,
            SERIAL,
            &indexed,
            &mut budget,
        )
        .expect_err("BRP cannot enter the PLD label boundary");
        assert_eq!(error.kind, ImportErrorKind::InvalidConfiguration);
        assert_eq!(budget, PldDecodeBudget::new(2, 1));
    }

    #[test]
    fn zero_limits_and_unsafe_expected_serial_are_configuration_errors() {
        let bytes = synthetic_pld(
            &[SignalFixture::new("Snore.2s", 2, vec![1, 2])],
            1,
            2.0,
            &recording_id(SERIAL),
        );
        let indexed = indexed_header(&bytes);
        let error = decode_pld_edf_with_limits(
            &bytes,
            ResmedSessionFileKind::Pld,
            SERIAL,
            &indexed,
            &mut PldDecodeBudget::default(),
            PldDecodeLimits {
                max_output_samples: 0,
                ..PldDecodeLimits::DEFAULT
            },
        )
        .expect_err("zero limit");
        assert_eq!(error.kind, ImportErrorKind::InvalidConfiguration);

        let error =
            decode_with_index(&bytes, "serial with space", &indexed).expect_err("unsafe serial");
        assert_eq!(error.kind, ImportErrorKind::InvalidConfiguration);
        assert!(!error.message.contains("serial with space"));
    }

    fn decode(bytes: &[u8], serial: &str) -> Result<DecodedPldFile, ImportError> {
        decode_with_index(bytes, serial, &indexed_header(bytes))
    }

    fn decode_with_index(
        bytes: &[u8],
        serial: &str,
        indexed: &ResmedEdfHeaderSummary,
    ) -> Result<DecodedPldFile, ImportError> {
        let mut budget = PldDecodeBudget::default();
        decode_pld_edf(
            bytes,
            ResmedSessionFileKind::Pld,
            serial,
            indexed,
            &mut budget,
        )
    }

    fn recording_id(serial: &str) -> String {
        format!("Startdate 23-JUL-2026 X X SRN={serial}")
    }

    fn indexed_header(bytes: &[u8]) -> ResmedEdfHeaderSummary {
        let header = Parser::new(PldDecodeLimits::DEFAULT.parser_limits())
            .parse_header(bytes)
            .expect("synthetic EDF header");
        let year = if (85..=99).contains(&header.start.year_two_digits) {
            2_000 + u16::from(header.start.year_two_digits)
        } else {
            header.start.year
        };
        let records = header.declared_record_count;
        let estimated_duration_millis = records.and_then(|records| {
            let records = u32::try_from(records).ok()?;
            let seconds = header.record_duration_seconds * f64::from(records);
            if !seconds.is_finite() || seconds < 0.0 || seconds > u64::MAX as f64 / 1_000.0 {
                return None;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            (seconds as u64).checked_mul(1_000)
        });
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
            header_bytes: u64::try_from(header.header_bytes).expect("header bytes fit"),
            signal_count: u16::try_from(header.signals.len()).expect("signal count fits"),
            declared_record_count: records.and_then(|records| u64::try_from(records).ok()),
            record_duration_seconds: header.record_duration_seconds,
            estimated_duration_millis,
        }
    }

    fn synthetic_pld(
        signals: &[SignalFixture],
        records: usize,
        record_duration_seconds: f64,
        recording_id: &str,
    ) -> Vec<u8> {
        assert!(!signals.is_empty());
        for signal in signals {
            assert_eq!(signal.samples.len(), signal.samples_per_record * records);
        }
        let header_bytes = 256 + signals.len() * 256;
        let mut bytes = Vec::new();
        bytes.extend(field("0", 8));
        bytes.extend(field("patient-private", 80));
        bytes.extend(field(recording_id, 80));
        bytes.extend(field("23.07.26", 8));
        bytes.extend(field("01.02.03", 8));
        bytes.extend(field(&header_bytes.to_string(), 8));
        bytes.extend(field("", 44));
        bytes.extend(field(&records.to_string(), 8));
        bytes.extend(field(&record_duration_seconds.to_string(), 8));
        bytes.extend(field(&signals.len().to_string(), 4));
        for signal in signals {
            bytes.extend(field(&signal.label, 16));
        }
        for _ in signals {
            bytes.extend(field("", 80));
        }
        for signal in signals {
            bytes.extend(field(&signal.physical_dimension, 8));
        }
        for signal in signals {
            bytes.extend(field(&edf_float(signal.physical_minimum), 8));
        }
        for signal in signals {
            bytes.extend(field(&edf_float(signal.physical_maximum), 8));
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

        for record in 0..records {
            for signal in signals {
                let start = record * signal.samples_per_record;
                let end = start + signal.samples_per_record;
                for sample in &signal.samples[start..end] {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
            }
        }
        bytes
    }

    fn field(value: &str, width: usize) -> Vec<u8> {
        assert!(
            value.len() <= width,
            "fixture field {value:?} exceeds {width} bytes"
        );
        let mut field = vec![b' '; width];
        field[..value.len()].copy_from_slice(value.as_bytes());
        field
    }

    fn overwrite_field(bytes: &mut [u8], offset: usize, width: usize, value: &str) {
        assert!(value.len() <= width);
        bytes[offset..offset + width].fill(b' ');
        bytes[offset..offset + value.len()].copy_from_slice(value.as_bytes());
    }

    fn edf_float(value: f64) -> String {
        let decimal = value.to_string();
        if decimal.len() <= 8 {
            decimal
        } else {
            format!("{value:e}")
        }
    }
}
