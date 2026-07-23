// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2025 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Selectively ported from OSCAR's ResMed BRP loading concepts:
// https://gitlab.com/CrimsonNape/OSCAR-code
// Upstream commit: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
// Relevant upstream file: oscar/SleepLib/loader_plugins/resmed_loader.cpp
// Modified: 2026-07-23

//! First bounded ResMed session-import slice: uncompressed BRP waveforms.
//!
//! This deliberately produces partial sessions. STR mask intervals, settings,
//! EVE/CSL events, PLD detail, and oximetry are not decoded by this slice.

#[cfg(test)]
use super::ResmedSessionIndex;
use super::{
    IMPORTER_ID, ResmedDeviceLocalTime, ResmedEdfHeaderSummary, ResmedImporter,
    ResmedSessionCandidate, ResmedSessionFile, ResmedSessionFileKind,
    index_session_candidates_from_inventory,
};
use crate::domain::{
    ChannelKind, ChannelMetadata, DeviceLocalDateTime, EdfSourceEncoding, ImportReport,
    ImportStatistics, ImportWarning, Session, SessionDataKind, SessionSummary, SessionTimestamp,
    WarningSeverity, WaveformSeries,
};
use crate::importer::{
    ImportClockContext, ImportError, ImportErrorKind, ImportOptions, ImportSource, Importer,
    SourceEntryKind,
};
use opap_channels::{ResmedFileKind, resmed_signal_prefix};
use opap_edf::{EdfHeader, Limits, Parser};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum bytes read from one uncompressed BRP file.
pub const RESMED_BRP_MAX_FILE_BYTES: usize = 256 * 1024 * 1024;

/// Maximum BRP files decoded by one import.
pub const RESMED_BRP_MAX_FILES_PER_IMPORT: usize = 4_096;

/// Maximum aggregate indexed BRP bytes accepted by one import.
pub const RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT: u64 = 512 * 1024 * 1024;

/// Maximum calibrated BRP samples materialized by one import.
pub const RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT: usize = 50_000_000;

const RESMED_BRP_MAX_SIGNALS: usize = 64;
const RESMED_BRP_MAX_RECORDS: usize = 1_000_000;
const RESMED_BRP_MAX_SIGNAL_RECORDS: usize = 8_000_000;
const RESMED_BRP_MAX_TOTAL_SAMPLES: usize = 50_000_000;
const RESMED_BRP_MAX_ANNOTATION_BYTES: usize = 1024 * 1024;
const RESMED_BRP_MAX_ANNOTATION_RECORDS: usize = 100_000;
const RESMED_BRP_MAX_ANNOTATIONS: usize = 100_000;
const RESMED_BRP_MAX_ANNOTATION_TEXT_BYTES: usize = 1024 * 1024;
const MAX_FIXED_UTC_OFFSET_SECONDS: u32 = 64_800;
const FLOW_RATE_CHANNEL: &str = "pap.series.flow_rate";

const BRP_LIMITS: Limits = Limits {
    max_signals: RESMED_BRP_MAX_SIGNALS,
    max_records: RESMED_BRP_MAX_RECORDS,
    max_signal_records: RESMED_BRP_MAX_SIGNAL_RECORDS,
    max_total_samples: RESMED_BRP_MAX_TOTAL_SAMPLES,
    max_annotation_bytes: RESMED_BRP_MAX_ANNOTATION_BYTES,
    max_annotation_records: RESMED_BRP_MAX_ANNOTATION_RECORDS,
    max_annotations: RESMED_BRP_MAX_ANNOTATIONS,
    max_annotation_text_bytes: RESMED_BRP_MAX_ANNOTATION_TEXT_BYTES,
};

pub(super) fn import_resmed_sessions(
    source: &dyn ImportSource,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let discovery = Importer::discover(&ResmedImporter, source)?.ok_or_else(|| {
        ImportError::new(
            ImportErrorKind::UnsupportedSource,
            "source is not a ResMed card",
        )
    })?;
    if discovery.device.machine.serial.trim().is_empty() {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "ResMed session import requires a non-empty machine serial number",
        ));
    }
    let machine_serial = discovery.device.machine.serial.trim().to_owned();

    let clock = options.clock_context.as_ref().ok_or_else(|| {
        ImportError::new(
            ImportErrorKind::InvalidConfiguration,
            "ResMed session import requires an explicit clock_context",
        )
    })?;
    validate_clock_context(clock)?;

    let current_device_local_time = resmed_local_from_device(clock.current_device_local_time)?;
    let index = index_session_candidates_from_inventory(
        source,
        &discovery.inventory,
        &current_device_local_time,
    )?;

    let files_discovered = discovery
        .inventory
        .entries
        .iter()
        .filter(|entry| entry.kind == SourceEntryKind::File)
        .count();
    let mut statistics = ImportStatistics {
        files_discovered: u64::try_from(files_discovered).unwrap_or(u64::MAX),
        ..ImportStatistics::default()
    };
    let mut warnings = discovery.warnings;
    warnings.extend(index.warnings);
    let mut sessions = Vec::new();
    let mut eligible_candidates = Vec::with_capacity(index.candidates.len());
    for candidate in &index.candidates {
        let prefiltered = options
            .sessions_not_before_unix_ms
            .is_some_and(|cutoff| candidate_ends_at_or_before(candidate, cutoff, clock));
        if prefiltered {
            statistics.sessions_skipped = statistics.sessions_skipped.saturating_add(1);
        } else {
            eligible_candidates.push(candidate);
        }
    }
    let mut import_budget = BrpImportBudget::from_candidates(&eligible_candidates)?;

    for candidate in eligible_candidates {
        let decoded = decode_candidate(
            source,
            candidate,
            &machine_serial,
            clock,
            &mut statistics,
            &mut import_budget,
        )?;
        warnings.extend(decoded.warnings);
        let Some(mut session) = decoded.session else {
            continue;
        };

        if options
            .sessions_not_before_unix_ms
            .is_some_and(|cutoff| session.end_time.normalized_utc_unix_ms <= cutoff)
        {
            statistics.sessions_skipped = statistics.sessions_skipped.saturating_add(1);
            continue;
        }

        if !options.include_waveforms {
            session.channels.clear();
            session.waveforms.clear();
        }
        warnings.push(warning(
            "resmed_partial_brp_session",
            "Imported BRP waveforms only; STR intervals, settings, events, PLD detail, and oximetry are unavailable",
            None,
            Some(&session.id),
        ));
        statistics.sessions_imported = statistics.sessions_imported.saturating_add(1);
        sessions.push(session);
    }

    Ok(ImportReport {
        schema_version: crate::domain::IMPORT_SCHEMA_VERSION,
        importer_id: IMPORTER_ID.to_owned(),
        device: discovery.device,
        sessions,
        warnings,
        statistics,
    })
}

struct CandidateDecode {
    session: Option<Session>,
    warnings: Vec<ImportWarning>,
}

struct DecodedBrpFile {
    local_start_ms: i64,
    local_end_ms: i64,
    fingerprint: [u8; 32],
    waveforms: Vec<WaveformSeries>,
    channels: Vec<ChannelMetadata>,
}

#[derive(Debug, Default)]
struct BrpImportBudget {
    actual_bytes_read: u64,
    output_samples: usize,
}

impl BrpImportBudget {
    fn from_candidates(candidates: &[&ResmedSessionCandidate]) -> Result<Self, ImportError> {
        validate_indexed_brp_resources(candidates)?;
        Ok(Self::default())
    }

    #[cfg(test)]
    fn from_index(index: &ResmedSessionIndex) -> Result<Self, ImportError> {
        let candidates: Vec<_> = index.candidates.iter().collect();
        Self::from_candidates(&candidates)
    }

    fn charge_actual_bytes(&mut self, additional: usize) -> Result<(), ImportError> {
        let additional = u64::try_from(additional).map_err(|_| {
            aggregate_limit_error("actual BRP byte count exceeds the supported integer range")
        })?;
        let next = self
            .actual_bytes_read
            .checked_add(additional)
            .ok_or_else(|| {
                aggregate_limit_error("aggregate actual BRP byte-count arithmetic overflowed")
            })?;
        if next > RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed BRP import reads at most {RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT} actual payload bytes"
            )));
        }
        self.actual_bytes_read = next;
        Ok(())
    }

    fn next_actual_read_limit(&self) -> Result<usize, ImportError> {
        let remaining = RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT
            .checked_sub(self.actual_bytes_read)
            .ok_or_else(|| {
                aggregate_limit_error("aggregate actual BRP byte accounting is inconsistent")
            })?;
        if remaining == 0 {
            return Err(aggregate_limit_error(format!(
                "ResMed BRP import reads at most {RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT} actual payload bytes"
            )));
        }
        let remaining = usize::try_from(remaining).unwrap_or(usize::MAX);
        Ok(RESMED_BRP_MAX_FILE_BYTES.min(remaining))
    }

    fn reserve_output_samples(&mut self, additional: usize) -> Result<(), ImportError> {
        let next = self.output_samples.checked_add(additional).ok_or_else(|| {
            aggregate_limit_error("aggregate BRP output-sample arithmetic overflowed")
        })?;
        if next > RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed BRP import accepts at most {RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT} calibrated output samples"
            )));
        }
        self.output_samples = next;
        Ok(())
    }
}

fn validate_indexed_brp_resources(
    candidates: &[&ResmedSessionCandidate],
) -> Result<(usize, u64), ImportError> {
    let mut unique_paths = BTreeSet::new();
    let mut files = 0usize;
    let mut bytes = 0u64;
    for file in candidates
        .iter()
        .copied()
        .flat_map(|candidate| &candidate.files)
        .filter(|file| file.kind == ResmedSessionFileKind::Brp)
    {
        if !unique_paths.insert(file.relative_path.as_str()) {
            continue;
        }
        files = files
            .checked_add(1)
            .ok_or_else(|| aggregate_limit_error("BRP file-count arithmetic overflowed"))?;
        if files > RESMED_BRP_MAX_FILES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed BRP import accepts at most {RESMED_BRP_MAX_FILES_PER_IMPORT} files"
            )));
        }
        bytes = bytes.checked_add(file.size_bytes).ok_or_else(|| {
            aggregate_limit_error("aggregate BRP byte-count arithmetic overflowed")
        })?;
        if bytes > RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed BRP import accepts at most {RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT} indexed bytes"
            )));
        }
    }
    Ok((files, bytes))
}

fn aggregate_limit_error(message: impl Into<String>) -> ImportError {
    ImportError::new(ImportErrorKind::SizeLimitExceeded, message)
}

fn decode_candidate(
    source: &dyn ImportSource,
    candidate: &ResmedSessionCandidate,
    machine_serial: &str,
    clock: &ImportClockContext,
    statistics: &mut ImportStatistics,
    import_budget: &mut BrpImportBudget,
) -> Result<CandidateDecode, ImportError> {
    let mut warnings = Vec::new();
    let mut files = Vec::new();

    for (file_ordinal, file) in candidate
        .files
        .iter()
        .filter(|file| file.kind == ResmedSessionFileKind::Brp)
        .enumerate()
    {
        match decode_brp_file(
            source,
            file,
            file_ordinal,
            machine_serial,
            clock,
            statistics,
            import_budget,
        ) {
            Ok(decoded) => {
                warnings.extend(decoded.1);
                if let Some(file) = decoded.0 {
                    files.push(file);
                }
            }
            Err(error) if error.kind == ImportErrorKind::SizeLimitExceeded => {
                return Err(error);
            }
            Err(error) => warnings.push(warning(
                "resmed_brp_not_decoded",
                format!("BRP detail was skipped: {error}"),
                Some(&file.relative_path),
                None,
            )),
        }
    }

    if files.is_empty() {
        warnings.push(warning(
            "resmed_candidate_not_imported",
            "Candidate has no trustworthy decoded BRP waveform data",
            None,
            None,
        ));
        return Ok(CandidateDecode {
            session: None,
            warnings,
        });
    }

    let mut waveforms = Vec::new();
    let mut channels = BTreeMap::<String, ChannelMetadata>::new();
    let mut file_fingerprints = Vec::new();
    let mut local_start_ms = i64::MAX;
    let mut local_end_ms = i64::MIN;
    for file in files {
        local_start_ms = local_start_ms.min(file.local_start_ms);
        local_end_ms = local_end_ms.max(file.local_end_ms);
        file_fingerprints.push(file.fingerprint.to_vec());
        for channel in file.channels {
            channels.entry(channel.id.clone()).or_insert(channel);
        }
        waveforms.extend(file.waveforms);
    }

    if waveforms.is_empty() || local_end_ms <= local_start_ms {
        warnings.push(warning(
            "resmed_candidate_not_imported",
            "Candidate had no supported, calibrated BRP signals and was not emitted as a session",
            None,
            None,
        ));
        return Ok(CandidateDecode {
            session: None,
            warnings,
        });
    }

    let source_key = session_source_key(
        &candidate.resmed_day,
        local_start_ms,
        local_end_ms,
        &file_fingerprints,
        &waveforms,
    );
    let id = opaque_key(
        "opap/resmed/session-id/v1",
        std::iter::once(source_key.as_bytes()),
    );
    let start_time = session_timestamp(local_start_ms, clock)?;
    let end_time = session_timestamp(local_end_ms, clock)?;
    let usage_ms = end_time
        .normalized_utc_unix_ms
        .checked_sub(start_time.normalized_utc_unix_ms)
        .and_then(|duration| u64::try_from(duration).ok())
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidConfiguration,
                "clock context produced an invalid session duration",
            )
        })?;

    for warning in &mut warnings {
        warning.session_id = Some(id.clone());
    }

    Ok(CandidateDecode {
        session: Some(Session {
            id,
            source_key,
            therapy_day: candidate.resmed_day.clone(),
            data_kind: SessionDataKind::Partial,
            start_time,
            end_time,
            slices: Vec::new(),
            channels: channels.into_values().collect(),
            waveforms,
            event_series: Vec::new(),
            settings: Vec::new(),
            summary: SessionSummary {
                usage_ms,
                metrics: Vec::new(),
            },
        }),
        warnings,
    })
}

fn decode_brp_file(
    source: &dyn ImportSource,
    indexed_file: &ResmedSessionFile,
    file_ordinal: usize,
    machine_serial: &str,
    clock: &ImportClockContext,
    statistics: &mut ImportStatistics,
    import_budget: &mut BrpImportBudget,
) -> Result<(Option<DecodedBrpFile>, Vec<ImportWarning>), ImportError> {
    if indexed_file
        .relative_path
        .to_ascii_lowercase()
        .ends_with(".gz")
    {
        return Err(ImportError::new(
            ImportErrorKind::UnsupportedOperation,
            "compressed BRP payloads are not supported by this import slice",
        )
        .at_path(&indexed_file.relative_path));
    }
    if indexed_file.size_bytes
        > u64::try_from(RESMED_BRP_MAX_FILE_BYTES).expect("file budget fits u64")
    {
        return Err(ImportError::new(
            ImportErrorKind::SizeLimitExceeded,
            format!("BRP file exceeds the {RESMED_BRP_MAX_FILE_BYTES}-byte import budget"),
        )
        .at_path(&indexed_file.relative_path));
    }

    let read_limit = import_budget.next_actual_read_limit()?;
    let bytes = source.read_file(&indexed_file.relative_path, read_limit)?;
    import_budget.charge_actual_bytes(bytes.len())?;
    if bytes.len() > RESMED_BRP_MAX_FILE_BYTES {
        return Err(ImportError::new(
            ImportErrorKind::SizeLimitExceeded,
            format!(
                "source adapter returned {} bytes for a BRP file, exceeding the {}-byte import budget",
                bytes.len(),
                RESMED_BRP_MAX_FILE_BYTES
            ),
        )
        .at_path(&indexed_file.relative_path));
    }
    statistics.files_read = statistics.files_read.saturating_add(1);
    statistics.bytes_read = statistics
        .bytes_read
        .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    if u64::try_from(bytes.len()).ok() != Some(indexed_file.size_bytes) {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "BRP file size changed after candidate indexing",
        )
        .at_path(&indexed_file.relative_path));
    }

    let parsed = Parser::new(BRP_LIMITS).parse(&bytes).map_err(|source| {
        ImportError::new(
            ImportErrorKind::InvalidData,
            format!("failed to decode complete BRP EDF: {source}"),
        )
        .at_path(&indexed_file.relative_path)
    })?;
    let summary = indexed_file.edf_header.as_ref().ok_or_else(|| {
        ImportError::new(
            ImportErrorKind::InvalidData,
            "candidate BRP file has no indexed EDF header summary",
        )
        .at_path(&indexed_file.relative_path)
    })?;
    validate_header_summary(parsed.header(), summary).map_err(|message| {
        ImportError::new(ImportErrorKind::InvalidData, message).at_path(&indexed_file.relative_path)
    })?;
    if parsed.header().is_discontinuous() {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "EDF+D BRP files are discontinuous and are not supported",
        )
        .at_path(&indexed_file.relative_path));
    }

    let mut warnings = Vec::new();
    match recording_serial(&parsed.header().recording_id) {
        Some(serial) if serial != machine_serial => {
            warnings.push(warning(
                "resmed_brp_serial_mismatch",
                "BRP machine identity did not match the selected card; file skipped",
                Some(&indexed_file.relative_path),
                None,
            ));
            return Ok((None, warnings));
        }
        Some(_) => {}
        None => warnings.push(warning(
            "resmed_brp_serial_missing",
            "BRP recording header has no SRN token; file imported without per-file identity verification",
            Some(&indexed_file.relative_path),
            None,
        )),
    }

    let local_start = device_local_from_resmed(&indexed_file.selected_start_time)?;
    let local_start_ms = local_unix_millis(&local_start).map_err(invalid_source_clock)?;
    let duration_ms = decoded_duration_millis(
        parsed.record_count(),
        parsed.header().record_duration_seconds,
    )
    .ok_or_else(|| {
        ImportError::new(
            ImportErrorKind::InvalidData,
            "decoded BRP duration is zero, non-finite, or outside timestamp range",
        )
        .at_path(&indexed_file.relative_path)
    })?;
    let local_end_ms = local_start_ms.checked_add(duration_ms).ok_or_else(|| {
        ImportError::new(
            ImportErrorKind::InvalidData,
            "decoded BRP end time exceeds the supported calendar range",
        )
        .at_path(&indexed_file.relative_path)
    })?;
    local_datetime_from_millis(local_end_ms).map_err(invalid_source_clock)?;

    let fingerprint = sha256(&bytes);
    let file_ordinal_bytes = u64::try_from(file_ordinal)
        .unwrap_or(u64::MAX)
        .to_be_bytes();
    let mut waveforms = Vec::new();
    let mut channels = Vec::new();
    for (signal_index, signal) in parsed.signals().iter().enumerate() {
        let label = signal.header.label.trim();
        if label.eq_ignore_ascii_case("Crc16") {
            continue;
        }
        let Some(channel) = resmed_signal_prefix(ResmedFileKind::Brp, label) else {
            warnings.push(warning(
                "unknown_resmed_brp_signal",
                format!("Unknown BRP signal ignored: {label}"),
                Some(&indexed_file.relative_path),
                None,
            ));
            continue;
        };
        if signal.header.samples_per_record == 0 {
            warnings.push(warning(
                "invalid_resmed_brp_signal",
                format!("BRP signal {label} has zero samples per record"),
                Some(&indexed_file.relative_path),
                None,
            ));
            continue;
        }
        let samples_per_record = u32::try_from(signal.header.samples_per_record).map_err(|_| {
            ImportError::new(
                ImportErrorKind::InvalidData,
                format!("BRP signal {label} sample cadence exceeds the supported range"),
            )
            .at_path(&indexed_file.relative_path)
        })?;
        let sample_interval_ms =
            parsed.header().record_duration_seconds * 1_000.0 / f64::from(samples_per_record);
        if !sample_interval_ms.is_finite() || sample_interval_ms <= 0.0 {
            warnings.push(warning(
                "invalid_resmed_brp_signal",
                format!("BRP signal {label} has an invalid sampling interval"),
                Some(&indexed_file.relative_path),
                None,
            ));
            continue;
        }

        let flow_scale = if channel.key.as_str() == FLOW_RATE_CHANNEL {
            60.0
        } else {
            1.0
        };
        let physical = match signal.physical_samples() {
            Ok(physical) => physical,
            Err(error) => {
                warnings.push(warning(
                    "invalid_resmed_brp_calibration",
                    format!("BRP signal {label} has invalid EDF calibration: {error}"),
                    Some(&indexed_file.relative_path),
                    None,
                ));
                continue;
            }
        };
        import_budget.reserve_output_samples(physical.len())?;
        let mut samples = Vec::with_capacity(physical.len());
        let mut valid = true;
        for value in physical {
            let normalized = value * flow_scale;
            #[allow(clippy::cast_possible_truncation)]
            let normalized = normalized as f32;
            if !normalized.is_finite() {
                valid = false;
                break;
            }
            samples.push(normalized);
        }
        if !valid {
            warnings.push(warning(
                "invalid_resmed_brp_calibration",
                format!("BRP signal {label} calibration produced a non-finite sample"),
                Some(&indexed_file.relative_path),
                None,
            ));
            continue;
        }

        let signal_index_bytes = u64::try_from(signal_index)
            .unwrap_or(u64::MAX)
            .to_be_bytes();
        let source_key = opaque_key(
            "opap/resmed/brp-waveform/v1",
            [
                fingerprint.as_slice(),
                file_ordinal_bytes.as_slice(),
                signal_index_bytes.as_slice(),
                channel.key.as_str().as_bytes(),
            ],
        );
        let unit = channel.unit.symbol();
        let channel_id = channel.key.as_str().to_owned();
        channels.push(ChannelMetadata {
            id: channel_id.clone(),
            label: channel.label.to_owned(),
            unit: (!unit.is_empty()).then(|| unit.to_owned()),
            kind: ChannelKind::Waveform,
        });
        waveforms.push(WaveformSeries {
            source_key,
            channel_id,
            start_time_unix_ms: normalize_millis(local_start_ms, clock)?,
            sample_interval_ms,
            samples,
            source_encoding: Some(EdfSourceEncoding {
                digital_minimum: signal.header.digital_minimum,
                digital_maximum: signal.header.digital_maximum,
                physical_minimum: signal.header.physical_minimum,
                physical_maximum: signal.header.physical_maximum,
                samples_per_record,
                record_duration_seconds: parsed.header().record_duration_seconds,
            }),
        });
    }

    Ok((
        Some(DecodedBrpFile {
            local_start_ms,
            local_end_ms,
            fingerprint,
            waveforms,
            channels,
        }),
        warnings,
    ))
}

fn recording_serial(recording_id: &str) -> Option<&str> {
    recording_id
        .split_ascii_whitespace()
        .filter_map(|token| token.strip_prefix("SRN="))
        .find(|serial| !serial.is_empty())
}

fn validate_header_summary(
    header: &EdfHeader,
    summary: &ResmedEdfHeaderSummary,
) -> Result<(), &'static str> {
    if u64::try_from(header.header_bytes).ok() != Some(summary.header_bytes)
        || u16::try_from(header.signals.len()).ok() != Some(summary.signal_count)
        || header
            .declared_record_count
            .and_then(|count| u64::try_from(count).ok())
            != summary.declared_record_count
        || header.record_duration_seconds.to_bits() != summary.record_duration_seconds.to_bits()
    {
        return Err("complete BRP header no longer matches its indexed summary");
    }
    if let Some(indexed_start) = &summary.start_time {
        let start = &header.start;
        let resmed_year = if (85..=99).contains(&start.year_two_digits) {
            2_000 + u16::from(start.year_two_digits)
        } else {
            start.year
        };
        if resmed_year != indexed_start.year
            || start.month != indexed_start.month
            || start.day != indexed_start.day
            || start.hour != indexed_start.hour
            || start.minute != indexed_start.minute
            || start.second != indexed_start.second
        {
            return Err("complete BRP start time no longer matches its indexed summary");
        }
    }
    Ok(())
}

fn candidate_ends_at_or_before(
    candidate: &ResmedSessionCandidate,
    cutoff: i64,
    clock: &ImportClockContext,
) -> bool {
    let Some(estimated_end) = &candidate.estimated_end_time else {
        return false;
    };
    let Ok(device_local) = device_local_from_resmed(estimated_end) else {
        return false;
    };
    let Ok(local_millis) = local_unix_millis(&device_local) else {
        return false;
    };
    normalize_millis(local_millis, clock).is_ok_and(|end| end <= cutoff)
}

fn decoded_duration_millis(record_count: usize, record_duration_seconds: f64) -> Option<i64> {
    let count = u32::try_from(record_count).ok()?;
    let millis = f64::from(count) * record_duration_seconds * 1_000.0;
    if !millis.is_finite() || millis <= 0.0 || millis > i64::MAX as f64 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(millis.round() as i64)
}

fn session_source_key(
    therapy_day: &str,
    local_start_ms: i64,
    local_end_ms: i64,
    file_fingerprints: &[Vec<u8>],
    waveforms: &[WaveformSeries],
) -> String {
    let mut parts = Vec::with_capacity(file_fingerprints.len() + waveforms.len() + 3);
    parts.push(therapy_day.as_bytes().to_vec());
    parts.push(local_start_ms.to_be_bytes().to_vec());
    parts.push(local_end_ms.to_be_bytes().to_vec());
    parts.extend(file_fingerprints.iter().cloned());
    parts.extend(
        waveforms
            .iter()
            .map(|waveform| waveform.source_key.as_bytes().to_vec()),
    );
    opaque_key(
        "opap/resmed/session-source/v1",
        parts.iter().map(Vec::as_slice),
    )
}

fn opaque_key<'a>(domain: &str, parts: impl IntoIterator<Item = &'a [u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    for part in parts {
        hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
        hasher.update(part);
    }
    format!("sha256:{}", hex(&hasher.finalize()))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn validate_clock_context(clock: &ImportClockContext) -> Result<(), ImportError> {
    local_unix_millis(&clock.current_device_local_time)
        .map_err(|message| ImportError::new(ImportErrorKind::InvalidConfiguration, message))?;
    if clock.applied_utc_offset_seconds.unsigned_abs() > MAX_FIXED_UTC_OFFSET_SECONDS {
        return Err(ImportError::new(
            ImportErrorKind::InvalidConfiguration,
            format!("applied_utc_offset_seconds must be within +/-{MAX_FIXED_UTC_OFFSET_SECONDS}"),
        ));
    }
    if clock
        .timezone_basis
        .as_ref()
        .is_some_and(|basis| basis.trim().is_empty())
    {
        return Err(ImportError::new(
            ImportErrorKind::InvalidConfiguration,
            "timezone_basis must be non-empty when supplied",
        ));
    }

    for boundary in [
        DeviceLocalDateTime {
            year: 1,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
        },
        DeviceLocalDateTime {
            year: u16::MAX,
            month: 12,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
            millisecond: 999,
        },
    ] {
        let local = local_unix_millis(&boundary).expect("fixed boundary is valid");
        normalize_millis(local, clock)?;
    }
    Ok(())
}

fn normalize_millis(local_millis: i64, clock: &ImportClockContext) -> Result<i64, ImportError> {
    let offset_millis = i64::from(clock.applied_utc_offset_seconds)
        .checked_mul(1_000)
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidConfiguration,
                "UTC offset millisecond conversion overflowed",
            )
        })?;
    local_millis
        .checked_add(clock.device_clock_correction_ms)
        .and_then(|value| value.checked_sub(offset_millis))
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidConfiguration,
                "device clock correction and UTC offset overflow timestamp arithmetic",
            )
        })
}

fn session_timestamp(
    local_millis: i64,
    clock: &ImportClockContext,
) -> Result<SessionTimestamp, ImportError> {
    let local = local_datetime_from_millis(local_millis).map_err(invalid_source_clock)?;
    Ok(SessionTimestamp {
        normalized_utc_unix_ms: normalize_millis(local_millis, clock)?,
        device_local_wall_time: format_local_time(&local),
        device_local: Some(local),
        applied_utc_offset_seconds: Some(clock.applied_utc_offset_seconds),
        device_clock_correction_ms: clock.device_clock_correction_ms,
        timezone_basis: clock.timezone_basis.clone(),
    })
}

fn resmed_local_from_device(
    value: DeviceLocalDateTime,
) -> Result<ResmedDeviceLocalTime, ImportError> {
    local_unix_millis(&value)
        .map_err(|message| ImportError::new(ImportErrorKind::InvalidConfiguration, message))?;
    Ok(ResmedDeviceLocalTime {
        wall_time: format_local_time(&value),
        year: value.year,
        month: value.month,
        day: value.day,
        hour: value.hour,
        minute: value.minute,
        second: value.second,
        millisecond: value.millisecond,
    })
}

fn device_local_from_resmed(
    value: &ResmedDeviceLocalTime,
) -> Result<DeviceLocalDateTime, ImportError> {
    let local = DeviceLocalDateTime {
        year: value.year,
        month: value.month,
        day: value.day,
        hour: value.hour,
        minute: value.minute,
        second: value.second,
        millisecond: value.millisecond,
    };
    local_unix_millis(&local).map_err(invalid_source_clock)?;
    Ok(local)
}

fn invalid_source_clock(message: &'static str) -> ImportError {
    ImportError::new(ImportErrorKind::InvalidData, message)
}

fn local_unix_millis(value: &DeviceLocalDateTime) -> Result<i64, &'static str> {
    if !valid_local_datetime(value) {
        return Err("device-local time is not a valid calendar time");
    }
    days_from_civil(value.year, value.month, value.day)
        .checked_mul(86_400_000)
        .and_then(|millis| {
            millis.checked_add(
                i64::from(value.hour) * 3_600_000
                    + i64::from(value.minute) * 60_000
                    + i64::from(value.second) * 1_000
                    + i64::from(value.millisecond),
            )
        })
        .ok_or("device-local calendar conversion overflowed")
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

fn local_datetime_from_millis(value: i64) -> Result<DeviceLocalDateTime, &'static str> {
    let days = value.div_euclid(86_400_000);
    let within_day = value.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days)?;
    Ok(DeviceLocalDateTime {
        year,
        month,
        day,
        hour: u8::try_from(within_day / 3_600_000).expect("hour is bounded"),
        minute: u8::try_from((within_day % 3_600_000) / 60_000).expect("minute is bounded"),
        second: u8::try_from((within_day % 60_000) / 1_000).expect("second is bounded"),
        millisecond: u16::try_from(within_day % 1_000).expect("millisecond is bounded"),
    })
}

fn civil_from_days(days: i64) -> Result<(u16, u8, u8), &'static str> {
    let zero_day = days
        .checked_add(719_468)
        .ok_or("device-local calendar conversion overflowed")?;
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
    Ok((
        u16::try_from(year).map_err(|_| "device-local end is outside the supported year range")?,
        u8::try_from(month).map_err(|_| "device-local month conversion failed")?,
        u8::try_from(day).map_err(|_| "device-local day conversion failed")?,
    ))
}

fn format_local_time(value: &DeviceLocalDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}",
        value.year,
        value.month,
        value.day,
        value.hour,
        value.minute,
        value.second,
        value.millisecond
    )
}

fn warning(
    code: impl Into<String>,
    message: impl Into<String>,
    relative_path: Option<&str>,
    session_id: Option<&str>,
) -> ImportWarning {
    ImportWarning {
        code: code.into(),
        severity: WarningSeverity::Warning,
        message: message.into(),
        relative_path: relative_path.map(str::to_owned),
        session_id: session_id.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionDataKind;
    use crate::importer::{SourceEntry, SourceInventory};
    use crate::resmed::{ResmedSessionFileScope, ResmedTimestampSource};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Clone, Default)]
    struct MemorySource {
        inventory: SourceInventory,
        files: BTreeMap<String, Vec<u8>>,
        full_reads: RefCell<BTreeMap<String, usize>>,
    }

    impl MemorySource {
        fn insert(&mut self, path: &str, bytes: Vec<u8>) {
            self.inventory.entries.push(SourceEntry {
                relative_path: path.to_owned(),
                kind: SourceEntryKind::File,
                size_bytes: u64::try_from(bytes.len()).expect("fixture length fits u64"),
            });
            self.inventory.total_file_bytes = self
                .inventory
                .total_file_bytes
                .saturating_add(u64::try_from(bytes.len()).expect("fixture length fits u64"));
            self.files.insert(path.to_owned(), bytes);
            self.inventory
                .entries
                .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        }

        fn full_read_count(&self, path: &str) -> usize {
            self.full_reads.borrow().get(path).copied().unwrap_or(0)
        }
    }

    impl ImportSource for MemorySource {
        fn inventory(&self) -> Result<SourceInventory, ImportError> {
            Ok(self.inventory.clone())
        }

        fn read_file(&self, relative_path: &str, max_bytes: usize) -> Result<Vec<u8>, ImportError> {
            let bytes = self.files.get(relative_path).ok_or_else(|| {
                ImportError::new(ImportErrorKind::Source, "fixture file is missing")
                    .at_path(relative_path)
            })?;
            *self
                .full_reads
                .borrow_mut()
                .entry(relative_path.to_owned())
                .or_default() += 1;
            if bytes.len() > max_bytes {
                return Err(ImportError::new(
                    ImportErrorKind::SizeLimitExceeded,
                    "fixture exceeds requested read limit",
                )
                .at_path(relative_path));
            }
            Ok(bytes.clone())
        }

        fn read_file_prefix(
            &self,
            relative_path: &str,
            max_bytes: usize,
        ) -> Result<Vec<u8>, ImportError> {
            let bytes = self.files.get(relative_path).ok_or_else(|| {
                ImportError::new(ImportErrorKind::Source, "fixture file is missing")
                    .at_path(relative_path)
            })?;
            Ok(bytes[..bytes.len().min(max_bytes)].to_vec())
        }
    }

    #[derive(Clone)]
    struct SignalFixture<'a> {
        label: &'a str,
        dimension: &'a str,
        physical_minimum: i32,
        physical_maximum: i32,
        digital_minimum: i32,
        digital_maximum: i32,
        samples_per_record: usize,
        samples: Vec<i16>,
    }

    impl<'a> SignalFixture<'a> {
        fn new(label: &'a str, samples_per_record: usize, samples: &[i16]) -> Self {
            Self {
                label,
                dimension: "unit",
                physical_minimum: -100,
                physical_maximum: 100,
                digital_minimum: -100,
                digital_maximum: 100,
                samples_per_record,
                samples: samples.to_vec(),
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

    fn field(value: &str, width: usize) -> Vec<u8> {
        assert!(value.len() <= width);
        let mut output = vec![b' '; width];
        output[..value.len()].copy_from_slice(value.as_bytes());
        output
    }

    fn synthetic_brp(
        signals: &[SignalFixture<'_>],
        record_count: usize,
        record_duration: &str,
    ) -> Vec<u8> {
        synthetic_brp_with_recording_id(
            signals,
            record_count,
            record_duration,
            "ResMed SRN=serial-123",
        )
    }

    fn synthetic_brp_with_recording_id(
        signals: &[SignalFixture<'_>],
        record_count: usize,
        record_duration: &str,
        recording_id: &str,
    ) -> Vec<u8> {
        let header_bytes = 256 + signals.len() * 256;
        let mut bytes = Vec::new();
        bytes.extend(field("0", 8));
        bytes.extend(field("patient", 80));
        bytes.extend(field(recording_id, 80));
        bytes.extend_from_slice(b"02.01.2622.00.00");
        bytes.extend(field(&header_bytes.to_string(), 8));
        bytes.extend(field("", 44));
        bytes.extend(field(&record_count.to_string(), 8));
        bytes.extend(field(record_duration, 8));
        bytes.extend(field(&signals.len().to_string(), 4));

        for signal in signals {
            bytes.extend(field(signal.label, 16));
        }
        for _ in signals {
            bytes.extend(field("", 80));
        }
        for signal in signals {
            bytes.extend(field(signal.dimension, 8));
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

    fn card_with_brp(bytes: Vec<u8>) -> MemorySource {
        let mut source = MemorySource::default();
        source.insert("STR.edf", b"signature only".to_vec());
        source.insert(
            "Identification.tgt",
            b"#SRN serial-123\n#PNA AirSense_10_AutoSet\n#PCD 37028\n".to_vec(),
        );
        source.insert("DATALOG/20260102_220000_BRP.edf", bytes);
        source
    }

    fn budget_file(size_bytes: u64) -> ResmedSessionFile {
        let local = ResmedDeviceLocalTime {
            wall_time: "2026-01-02T22:00:00.000".to_owned(),
            year: 2026,
            month: 1,
            day: 2,
            hour: 22,
            minute: 0,
            second: 0,
            millisecond: 0,
        };
        ResmedSessionFile {
            relative_path: "DATALOG/budget_BRP.edf".to_owned(),
            size_bytes,
            kind: ResmedSessionFileKind::Brp,
            scope: ResmedSessionFileScope::Session,
            filename_start_time: local.clone(),
            edf_header: None,
            selected_start_time: local,
            timestamp_source: ResmedTimestampSource::Filename,
        }
    }

    fn budget_index(files: Vec<ResmedSessionFile>) -> ResmedSessionIndex {
        let start_time = files
            .first()
            .expect("budget fixture has a file")
            .selected_start_time
            .clone();
        ResmedSessionIndex {
            schema_version: super::super::RESMED_SESSION_INDEX_SCHEMA_VERSION,
            candidates: vec![ResmedSessionCandidate {
                id: "budget-candidate".to_owned(),
                start_time,
                estimated_end_time: None,
                resmed_day: "2026-01-02".to_owned(),
                files,
            }],
            warnings: Vec::new(),
        }
    }

    fn clock() -> ImportClockContext {
        ImportClockContext {
            current_device_local_time: DeviceLocalDateTime {
                year: 2030,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
            },
            applied_utc_offset_seconds: 7 * 60 * 60,
            device_clock_correction_ms: 250,
            timezone_basis: Some("fixed:+07:00".to_owned()),
        }
    }

    fn options() -> ImportOptions {
        ImportOptions {
            clock_context: Some(clock()),
            ..ImportOptions::default()
        }
    }

    fn simple_flow() -> Vec<u8> {
        synthetic_brp(
            &[SignalFixture::new("Flow", 2, &[-100, 0, 50, 100]).calibration(-2, 2, -100, 100)],
            2,
            "1",
        )
    }

    #[test]
    fn imports_brp_with_fixed_plus_seven_clock_and_full_affine_flow_scaling() {
        let signals = [
            SignalFixture::new("fLoW.40mS extra", 2, &[-100, 0, 50, 100])
                .calibration(-2, 2, -100, 100),
            SignalFixture::new("pReSs.40MS x", 4, &[0, 25, 50, 100, 0, 25, 50, 100])
                .calibration(5, 15, 0, 100),
        ];
        let report =
            import_resmed_sessions(&card_with_brp(synthetic_brp(&signals, 2, "1")), &options())
                .expect("import BRP");

        assert_eq!(report.sessions.len(), 1);
        let session = &report.sessions[0];
        assert_eq!(session.data_kind, SessionDataKind::Partial);
        assert_eq!(session.therapy_day, "2026-01-02");
        assert_eq!(
            session.start_time.device_local_wall_time,
            "2026-01-02T22:00:00.000"
        );
        assert_eq!(
            session.end_time.device_local_wall_time,
            "2026-01-02T22:00:02.000"
        );
        let raw_start = local_unix_millis(
            session
                .start_time
                .device_local
                .as_ref()
                .expect("structured local time"),
        )
        .expect("valid local time");
        assert_eq!(
            session.start_time.normalized_utc_unix_ms,
            raw_start - 7 * 60 * 60 * 1_000 + 250
        );
        assert_eq!(session.summary.usage_ms, 2_000);
        assert!(session.settings.is_empty());
        assert!(session.event_series.is_empty());
        assert!(session.slices.is_empty());

        let flow = session
            .waveforms
            .iter()
            .find(|series| series.channel_id == FLOW_RATE_CHANNEL)
            .expect("flow series");
        assert_eq!(flow.samples, vec![-120.0, 0.0, 60.0, 120.0]);
        assert_eq!(flow.sample_interval_ms, 500.0);
        let pressure = session
            .waveforms
            .iter()
            .find(|series| series.channel_id == "pap.series.mask_pressure_high_rate")
            .expect("pressure series");
        assert_eq!(
            pressure.samples,
            vec![5.0, 7.5, 10.0, 15.0, 5.0, 7.5, 10.0, 15.0]
        );
        assert_eq!(pressure.sample_interval_ms, 250.0);
        assert_eq!(
            pressure
                .source_encoding
                .expect("source encoding")
                .samples_per_record,
            4
        );
        assert!(report.warnings.iter().any(|warning| {
            warning.code == "resmed_partial_brp_session"
                && warning.session_id.as_deref() == Some(&session.id)
        }));
    }

    #[test]
    fn resmed_century_repair_keeps_85_header_and_index_summary_consistent() {
        let mut bytes = simple_flow();
        bytes[168..184].copy_from_slice(b"02.01.8522.00.00");
        let report = import_resmed_sessions(&card_with_brp(bytes), &options())
            .expect("ResMed repairs EDF year 85 to 2085 consistently");

        assert_eq!(report.sessions.len(), 1);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "edf_header_time_in_future")
        );
    }

    #[test]
    fn preserves_duplicate_registered_signals_with_distinct_opaque_keys() {
        let signals = [
            SignalFixture::new("Flow", 1, &[0, 10]),
            SignalFixture::new("FLOW second copy", 1, &[20, 30]),
        ];
        let report =
            import_resmed_sessions(&card_with_brp(synthetic_brp(&signals, 2, "1")), &options())
                .expect("import duplicate signals");
        let session = &report.sessions[0];

        assert_eq!(session.waveforms.len(), 2);
        assert_eq!(session.channels.len(), 1);
        assert_ne!(
            session.waveforms[0].source_key,
            session.waveforms[1].source_key
        );
        assert!(session.id.starts_with("sha256:"));
        assert!(session.source_key.starts_with("sha256:"));
        assert_eq!(session.id.len(), 71);
        assert!(!session.id.contains("serial-123"));
        assert!(!session.source_key.contains("DATALOG"));
    }

    #[test]
    fn ignores_crc_and_warns_for_unknown_brp_labels() {
        let signals = [
            SignalFixture::new("Crc16", 1, &[0, 0]),
            SignalFixture::new("vendor mystery", 1, &[1, 2]),
            SignalFixture::new("Flow", 1, &[3, 4]),
        ];
        let report =
            import_resmed_sessions(&card_with_brp(synthetic_brp(&signals, 2, "1")), &options())
                .expect("import known signal");

        assert_eq!(report.sessions[0].waveforms.len(), 1);
        let unknown: Vec<_> = report
            .warnings
            .iter()
            .filter(|warning| warning.code == "unknown_resmed_brp_signal")
            .collect();
        assert_eq!(unknown.len(), 1);
        assert!(unknown[0].message.contains("vendor mystery"));
        assert!(report.warnings.iter().all(|warning| {
            warning.code != "unknown_resmed_brp_signal" || !warning.message.contains("Crc16")
        }));
    }

    #[test]
    fn include_waveforms_false_keeps_the_valid_partial_session_without_series() {
        let mut options = options();
        options.include_waveforms = false;
        let report = import_resmed_sessions(&card_with_brp(simple_flow()), &options)
            .expect("validate but omit waveforms");

        assert_eq!(report.sessions.len(), 1);
        assert!(report.sessions[0].waveforms.is_empty());
        assert!(report.sessions[0].channels.is_empty());
        assert_eq!(report.sessions[0].summary.usage_ms, 2_000);
    }

    #[test]
    fn sessions_not_before_uses_the_exclusive_end_boundary() {
        let source = card_with_brp(simple_flow());
        let baseline =
            import_resmed_sessions(&source, &options()).expect("baseline session import");
        let end = baseline.sessions[0].end_time.normalized_utc_unix_ms;

        let mut at_end = options();
        at_end.sessions_not_before_unix_ms = Some(end);
        let skipped = import_resmed_sessions(&source, &at_end).expect("cutoff import");
        assert!(skipped.sessions.is_empty());
        assert_eq!(skipped.statistics.sessions_skipped, 1);

        let mut before_end = options();
        before_end.sessions_not_before_unix_ms = Some(end - 1);
        let included = import_resmed_sessions(&source, &before_end).expect("boundary import");
        assert_eq!(included.sessions.len(), 1);
    }

    #[test]
    fn cutoff_prefilter_skips_known_old_end_before_budgeting_or_payload_read() {
        const OLD_PATH: &str = "DATALOG/20260101_220000_BRP.edf";
        const ACTIVE_PATH: &str = "DATALOG/20260102_220000_BRP.edf";

        let active = simple_flow();
        let active_bytes = u64::try_from(active.len()).expect("fixture length fits u64");
        let mut old = simple_flow();
        old[168..184].copy_from_slice(b"01.01.2622.00.00");
        let mut source = card_with_brp(active);
        source.insert(OLD_PATH, old);
        source
            .inventory
            .entries
            .iter_mut()
            .find(|entry| entry.relative_path == OLD_PATH)
            .expect("old inventory entry")
            .size_bytes = RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT + 1;

        let clock_context = clock();
        let old_end = DeviceLocalDateTime {
            year: 2026,
            month: 1,
            day: 1,
            hour: 22,
            minute: 0,
            second: 2,
            millisecond: 0,
        };
        let cutoff = normalize_millis(
            local_unix_millis(&old_end).expect("valid old end"),
            &clock_context,
        )
        .expect("normalize cutoff");
        let mut import_options = options();
        import_options.sessions_not_before_unix_ms = Some(cutoff);

        let report = import_resmed_sessions(&source, &import_options)
            .expect("old candidate is filtered before aggregate preflight");

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.statistics.sessions_skipped, 1);
        assert_eq!(report.statistics.bytes_read, active_bytes);
        assert_eq!(source.full_read_count(OLD_PATH), 0);
        assert_eq!(source.full_read_count(ACTIVE_PATH), 1);
    }

    #[test]
    fn corrupt_brp_payload_warns_and_never_creates_a_phantom_session() {
        let mut corrupt = simple_flow();
        corrupt.pop();
        let report = import_resmed_sessions(&card_with_brp(corrupt), &options())
            .expect("candidate corruption is non-fatal");

        assert!(report.sessions.is_empty());
        assert_eq!(report.statistics.sessions_imported, 0);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_brp_not_decoded")
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_candidate_not_imported")
        );
    }

    #[test]
    fn mismatched_brp_serial_skips_the_file_without_disclosing_either_serial() {
        let bytes = synthetic_brp_with_recording_id(
            &[SignalFixture::new("Flow", 1, &[0, 1])],
            2,
            "1",
            "ResMed SRN=another-secret",
        );
        let report = import_resmed_sessions(&card_with_brp(bytes), &options())
            .expect("identity mismatch is a per-file diagnostic");

        assert!(report.sessions.is_empty());
        let mismatch = report
            .warnings
            .iter()
            .find(|warning| warning.code == "resmed_brp_serial_mismatch")
            .expect("stable mismatch warning");
        assert!(report.warnings.iter().all(|warning| {
            !warning.message.contains("serial-123") && !warning.message.contains("another-secret")
        }));
        assert!(mismatch.message.contains("file skipped"));
    }

    #[test]
    fn missing_brp_serial_warns_but_does_not_hide_valid_waveforms() {
        let bytes = synthetic_brp_with_recording_id(
            &[SignalFixture::new("Flow", 1, &[0, 1])],
            2,
            "1",
            "ResMed recording",
        );
        let report = import_resmed_sessions(&card_with_brp(bytes), &options())
            .expect("missing per-file serial remains non-fatal");

        assert_eq!(report.sessions.len(), 1);
        let warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "resmed_brp_serial_missing")
            .expect("stable missing-SRN warning");
        assert_eq!(
            warning.session_id.as_deref(),
            Some(report.sessions[0].id.as_str())
        );
    }

    #[test]
    fn source_and_session_identifiers_are_stable_across_reimports() {
        let source = card_with_brp(simple_flow());
        let first = import_resmed_sessions(&source, &options()).expect("first import");
        let second = import_resmed_sessions(&source, &options()).expect("second import");

        assert_eq!(first.sessions[0].id, second.sessions[0].id);
        assert_eq!(first.sessions[0].source_key, second.sessions[0].source_key);
        assert_eq!(
            first.sessions[0].waveforms[0].source_key,
            second.sessions[0].waveforms[0].source_key
        );
    }

    #[test]
    fn absent_or_invalid_clock_context_is_a_fatal_configuration_error() {
        let source = card_with_brp(simple_flow());
        let absent = import_resmed_sessions(&source, &ImportOptions::default())
            .expect_err("clock is required");
        assert_eq!(absent.kind, ImportErrorKind::InvalidConfiguration);

        let mut invalid_calendar = options();
        invalid_calendar
            .clock_context
            .as_mut()
            .expect("clock")
            .current_device_local_time
            .day = 32;
        let calendar =
            import_resmed_sessions(&source, &invalid_calendar).expect_err("calendar must be valid");
        assert_eq!(calendar.kind, ImportErrorKind::InvalidConfiguration);

        let mut invalid_offset = options();
        invalid_offset
            .clock_context
            .as_mut()
            .expect("clock")
            .applied_utc_offset_seconds = 64_801;
        let offset =
            import_resmed_sessions(&source, &invalid_offset).expect_err("offset must be bounded");
        assert_eq!(offset.kind, ImportErrorKind::InvalidConfiguration);

        let mut overflowing_correction = options();
        overflowing_correction
            .clock_context
            .as_mut()
            .expect("clock")
            .device_clock_correction_ms = i64::MAX;
        let correction = import_resmed_sessions(&source, &overflowing_correction)
            .expect_err("correction arithmetic must be checked");
        assert_eq!(correction.kind, ImportErrorKind::InvalidConfiguration);
    }

    #[test]
    fn fixed_offset_accepts_storage_v8_boundaries_inclusively() {
        let mut context = clock();
        context.applied_utc_offset_seconds = 64_800;
        validate_clock_context(&context).expect("positive boundary");
        context.applied_utc_offset_seconds = -64_800;
        validate_clock_context(&context).expect("negative boundary");
        context.applied_utc_offset_seconds = -64_801;
        assert_eq!(
            validate_clock_context(&context)
                .expect_err("outside storage boundary")
                .kind,
            ImportErrorKind::InvalidConfiguration
        );
    }

    #[test]
    fn aggregate_brp_budgets_fail_closed_before_large_allocations() {
        let file = budget_file(1);
        let mut duplicated_candidate = budget_index(vec![file.clone()]);
        duplicated_candidate
            .candidates
            .push(duplicated_candidate.candidates[0].clone());
        let duplicated_candidates: Vec<_> = duplicated_candidate.candidates.iter().collect();
        assert_eq!(
            validate_indexed_brp_resources(&duplicated_candidates)
                .expect("same indexed path is counted once"),
            (1, 1)
        );

        let too_many_files = budget_index(
            (0..=RESMED_BRP_MAX_FILES_PER_IMPORT)
                .map(|index| {
                    let mut unique = file.clone();
                    unique.relative_path = format!("DATALOG/{index:04}_BRP.edf");
                    unique
                })
                .collect(),
        );
        assert_eq!(
            BrpImportBudget::from_index(&too_many_files)
                .expect_err("file budget")
                .kind,
            ImportErrorKind::SizeLimitExceeded
        );

        let mut large_file = file;
        large_file.size_bytes =
            u64::try_from(RESMED_BRP_MAX_FILE_BYTES).expect("file budget fits u64");
        let too_many_bytes = budget_index(
            (0..3)
                .map(|index| {
                    let mut unique = large_file.clone();
                    unique.relative_path = format!("DATALOG/large-{index}_BRP.edf");
                    unique
                })
                .collect(),
        );
        assert_eq!(
            BrpImportBudget::from_index(&too_many_bytes)
                .expect_err("aggregate byte budget")
                .kind,
            ImportErrorKind::SizeLimitExceeded
        );

        let aggregate_byte_limit = usize::try_from(RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT)
            .expect("aggregate byte budget fits usize");
        let mut actual = BrpImportBudget::default();
        actual
            .charge_actual_bytes(aggregate_byte_limit)
            .expect("inclusive actual-byte boundary");
        assert_eq!(
            actual
                .charge_actual_bytes(1)
                .expect_err("actual byte budget")
                .kind,
            ImportErrorKind::SizeLimitExceeded
        );
        assert_eq!(
            actual
                .next_actual_read_limit()
                .expect_err("zero remainder fails before reading")
                .kind,
            ImportErrorKind::SizeLimitExceeded
        );
        let mut short_remainder = BrpImportBudget::default();
        short_remainder
            .charge_actual_bytes(aggregate_byte_limit - 7)
            .expect("leave a seven-byte remainder");
        assert_eq!(
            short_remainder
                .next_actual_read_limit()
                .expect("bounded next read"),
            7
        );

        let mut output = BrpImportBudget::default();
        output
            .reserve_output_samples(RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT)
            .expect("inclusive sample boundary");
        assert_eq!(
            output
                .reserve_output_samples(1)
                .expect_err("sample budget")
                .kind,
            ImportErrorKind::SizeLimitExceeded
        );

        let source = card_with_brp(simple_flow());
        let clock_context = clock();
        let current = resmed_local_from_device(clock_context.current_device_local_time)
            .expect("valid fixture clock");
        let inventory = source.inventory().expect("fixture inventory");
        let index = index_session_candidates_from_inventory(&source, &inventory, &current)
            .expect("fixture candidate");
        let mut exhausted_actual = BrpImportBudget::default();
        exhausted_actual
            .charge_actual_bytes(aggregate_byte_limit)
            .expect("fill actual byte budget");
        let mut statistics = ImportStatistics::default();
        let fatal_actual = decode_candidate(
            &source,
            &index.candidates[0],
            "serial-123",
            &clock_context,
            &mut statistics,
            &mut exhausted_actual,
        )
        .err()
        .expect("aggregate actual-byte limit propagates");
        assert_eq!(fatal_actual.kind, ImportErrorKind::SizeLimitExceeded);
        assert_eq!(source.full_read_count("DATALOG/20260102_220000_BRP.edf"), 0);

        let mut exhausted_output = BrpImportBudget::default();
        exhausted_output
            .reserve_output_samples(RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT)
            .expect("fill aggregate output budget");
        let mut statistics = ImportStatistics::default();
        let fatal_output = decode_candidate(
            &source,
            &index.candidates[0],
            "serial-123",
            &clock_context,
            &mut statistics,
            &mut exhausted_output,
        )
        .err()
        .expect("aggregate output limit propagates");
        assert_eq!(fatal_output.kind, ImportErrorKind::SizeLimitExceeded);

        let oversized_file = budget_file(
            u64::try_from(RESMED_BRP_MAX_FILE_BYTES).expect("file budget fits u64") + 1,
        );
        let mut oversized_index = budget_index(vec![oversized_file]);
        let mut decode_budget =
            BrpImportBudget::from_index(&oversized_index).expect("aggregate budget still fits");
        let candidate = oversized_index.candidates.remove(0);
        let mut statistics = ImportStatistics::default();
        let fatal = decode_candidate(
            &MemorySource::default(),
            &candidate,
            "serial-123",
            &clock(),
            &mut statistics,
            &mut decode_budget,
        )
        .err()
        .expect("per-file size limit propagates");
        assert_eq!(fatal.kind, ImportErrorKind::SizeLimitExceeded);
    }

    #[test]
    fn old_serialized_import_options_remain_readable_without_a_clock() {
        let decoded: ImportOptions = serde_json::from_str(
            r#"{"sessions_not_before_unix_ms":null,"include_waveforms":true}"#,
        )
        .expect("read legacy options");
        assert_eq!(decoded, ImportOptions::default());
        assert!(decoded.clock_context.is_none());
    }

    #[test]
    fn empty_machine_serial_is_fatal_before_session_indexing() {
        let mut source = card_with_brp(simple_flow());
        source.insert("Identification.tgt", b"#SRN   \n".to_vec());
        let error = import_resmed_sessions(&source, &options()).expect_err("serial is required");
        assert_eq!(error.kind, ImportErrorKind::InvalidData);
        assert!(error.message.contains("serial"));
    }
}
