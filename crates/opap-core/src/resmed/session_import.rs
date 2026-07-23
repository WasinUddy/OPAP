// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2025 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Selectively ported from OSCAR's ResMed BRP and SAD/SA2 loading concepts:
// https://gitlab.com/CrimsonNape/OSCAR-code
// Upstream commit: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
// Relevant upstream file: oscar/SleepLib/loader_plugins/resmed_loader.cpp
// Modified: 2026-07-23

//! Bounded ResMed detail import: uncompressed BRP waveforms plus SAD/SA2
//! oximetry.
//!
//! Serial-verified STR mask intervals produce source-selected therapy slices,
//! including explicitly identified bounded repairs and summary-only sessions
//! when no trustworthy BRP waveform is present. Settings, EVE/CSL events, and
//! PLD detail are not yet attached by this slice. Oximetry remains restricted
//! to candidates with trustworthy decoded BRP therapy data.

use super::str::StrBoundaryRepair;
use super::{
    IMPORTER_ID, ResmedDeviceLocalTime, ResmedEdfHeaderSummary, ResmedImporter,
    ResmedSessionAnchor, ResmedSessionCandidate, ResmedSessionFile, ResmedSessionFileKind,
    index_session_candidates_from_inventory_for_machine,
};
#[cfg(test)]
use super::{ResmedSessionIndex, index_session_candidates_from_inventory};
use crate::domain::{
    ChannelKind, ChannelMetadata, DeviceLocalDateTime, EdfSourceEncoding, ImportReport,
    ImportStatistics, ImportWarning, Session, SessionDataKind, SessionSummary, SessionTimestamp,
    TherapySlice, TherapySliceState, WarningSeverity, WaveformSeries,
};
use crate::importer::{
    ImportClockContext, ImportError, ImportErrorKind, ImportOptions, ImportSource, Importer,
    SourceEntryKind,
};
use opap_channels::{ResmedFileKind, resmed_signal_prefix};
use opap_edf::{EdfFile, EdfHeader, Limits, Parser, Signal};
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum bytes read from one uncompressed BRP, SAD, or SA2 file.
///
/// The BRP-prefixed name is retained as part of the existing public API.
pub const RESMED_BRP_MAX_FILE_BYTES: usize = 256 * 1024 * 1024;

/// Maximum aggregate BRP, SAD, and SA2 files decoded by one import.
///
/// The BRP-prefixed name is retained as part of the existing public API.
pub const RESMED_BRP_MAX_FILES_PER_IMPORT: usize = 4_096;

/// Maximum aggregate indexed BRP, SAD, and SA2 bytes accepted by one import.
///
/// The BRP-prefixed name is retained as part of the existing public API.
pub const RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT: u64 = 512 * 1024 * 1024;

/// Maximum calibrated BRP and oximetry samples materialized by one import.
///
/// The BRP-prefixed name is retained as part of the existing public API.
pub const RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT: usize = 50_000_000;

const RESMED_BRP_MAX_SIGNALS: usize = 64;
const RESMED_BRP_MAX_RECORDS: usize = 1_000_000;
const RESMED_BRP_MAX_SIGNAL_RECORDS: usize = 8_000_000;
const RESMED_BRP_MAX_TOTAL_SAMPLES: usize = 50_000_000;
const RESMED_BRP_MAX_ANNOTATION_BYTES: usize = 1024 * 1024;
const RESMED_BRP_MAX_ANNOTATION_RECORDS: usize = 100_000;
const RESMED_BRP_MAX_ANNOTATIONS: usize = 100_000;
const RESMED_BRP_MAX_ANNOTATION_TEXT_BYTES: usize = 1024 * 1024;
const RESMED_DETAIL_MAX_OUTPUT_SERIES_PER_IMPORT: usize = 100_000;
const MAX_FIXED_UTC_OFFSET_SECONDS: u32 = 64_800;
const MAX_TIMEZONE_BASIS_BYTES: usize = 256;
const FLOW_RATE_CHANNEL: &str = "pap.series.flow_rate";
const PULSE_RATE_CHANNEL: &str = "oximetry.series.pulse_rate";
const OXYGEN_SATURATION_CHANNEL: &str = "oximetry.series.oxygen_saturation";

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

struct CountingImportSource<'a> {
    inner: &'a dyn ImportSource,
    files_read: RefCell<BTreeSet<String>>,
    bytes_read: Cell<u64>,
}

impl<'a> CountingImportSource<'a> {
    fn new(inner: &'a dyn ImportSource) -> Self {
        Self {
            inner,
            files_read: RefCell::new(BTreeSet::new()),
            bytes_read: Cell::new(0),
        }
    }

    fn record_read(&self, relative_path: &str, byte_count: usize) {
        self.files_read
            .borrow_mut()
            .insert(relative_path.to_owned());
        self.bytes_read.set(
            self.bytes_read
                .get()
                .saturating_add(u64::try_from(byte_count).unwrap_or(u64::MAX)),
        );
    }

    fn statistics(&self) -> (u64, u64) {
        (
            u64::try_from(self.files_read.borrow().len()).unwrap_or(u64::MAX),
            self.bytes_read.get(),
        )
    }
}

impl ImportSource for CountingImportSource<'_> {
    fn inventory(&self) -> Result<crate::SourceInventory, ImportError> {
        self.inner.inventory()
    }

    fn read_file(&self, relative_path: &str, max_bytes: usize) -> Result<Vec<u8>, ImportError> {
        let bytes = self.inner.read_file(relative_path, max_bytes)?;
        self.record_read(relative_path, bytes.len());
        Ok(bytes)
    }

    fn read_file_prefix(
        &self,
        relative_path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, ImportError> {
        let bytes = self.inner.read_file_prefix(relative_path, max_bytes)?;
        self.record_read(relative_path, bytes.len());
        Ok(bytes)
    }
}

pub(super) fn import_resmed_sessions(
    source: &dyn ImportSource,
    options: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    let counted_source = CountingImportSource::new(source);
    let source: &dyn ImportSource = &counted_source;
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
    let index = index_session_candidates_from_inventory_for_machine(
        source,
        &discovery.inventory,
        &machine_serial,
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
    let mut import_budget = DetailImportBudget::from_brp_candidates(&eligible_candidates)?;
    let mut anchored_candidates = Vec::new();
    for candidate in eligible_candidates.iter().copied() {
        let decoded = decode_brp_candidate(
            source,
            candidate,
            &machine_serial,
            clock,
            &mut statistics,
            &mut import_budget,
        )?;
        if decoded.session.is_some() {
            anchored_candidates.push((candidate, decoded));
        } else {
            warnings.extend(decoded.warnings);
        }
    }

    for (candidate, mut decoded) in anchored_candidates {
        let mut session = decoded
            .session
            .take()
            .expect("anchored candidate contains an imported session");
        if decoded.brp_anchored {
            attach_candidate_oximetry(
                source,
                candidate,
                &machine_serial,
                clock,
                &mut session,
                &mut decoded.warnings,
                &mut statistics,
                &mut import_budget,
            )?;
        }
        warnings.extend(decoded.warnings);

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
        if let ResmedSessionAnchor::StrMask {
            repair: Some(repair),
            ..
        } = &candidate.anchor
        {
            let (code, message) = match repair {
                StrBoundaryRepair::SlotZeroContinuation => (
                    "resmed_str_slot_zero_continuation_repair",
                    "STR session start was selected as local noon because the slot-zero continuing mask-on was encoded as zero",
                ),
                StrBoundaryRepair::HistoricalTrailingNoon => (
                    "resmed_str_historical_trailing_noon_repair",
                    "STR session end was bounded at the following local noon because the historical source mask-off value was absent",
                ),
            };
            warnings.push(warning(code, message, None, Some(&session.id)));
        }
        match (&candidate.anchor, decoded.brp_anchored) {
            (ResmedSessionAnchor::StrMask { .. }, true) => warnings.push(warning(
                "resmed_partial_str_session",
                "Imported the selected STR therapy interval with bounded BRP waveforms and any trustworthy SAD/SA2 oximetry; settings, day summaries, EVE/CSL events, and PLD detail are unavailable",
                None,
                Some(&session.id),
            )),
            (ResmedSessionAnchor::StrMask { .. }, false) => warnings.push(warning(
                "resmed_str_boundary_only_session",
                "Imported the selected STR therapy interval without trustworthy BRP waveform data; settings, day summaries, EVE/CSL events, and PLD detail are unavailable",
                None,
                Some(&session.id),
            )),
            (ResmedSessionAnchor::DetailFallback, _) => warnings.push(warning(
                "resmed_partial_brp_session",
                "Imported bounded BRP waveforms and any trustworthy SAD/SA2 oximetry; STR intervals, settings, EVE/CSL events, and PLD detail are unavailable",
                None,
                Some(&session.id),
            )),
        }
        statistics.sessions_imported = statistics.sessions_imported.saturating_add(1);
        sessions.push(session);
    }
    (statistics.files_read, statistics.bytes_read) = counted_source.statistics();

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
    brp_anchored: bool,
}

struct DecodedDetailFile {
    local_start_ms: i64,
    local_end_ms: i64,
    waveforms: Vec<WaveformSeries>,
    channels: Vec<ChannelMetadata>,
}

struct ParsedDetailEdf {
    parsed: EdfFile,
    local_start_ms: i64,
    local_end_ms: i64,
    fingerprint: [u8; 32],
}

#[derive(Debug, Default)]
struct DetailImportBudget {
    indexed_files: usize,
    indexed_bytes: u64,
    actual_bytes_read: u64,
    output_samples: usize,
    output_series: usize,
}

impl DetailImportBudget {
    fn from_brp_candidates(candidates: &[&ResmedSessionCandidate]) -> Result<Self, ImportError> {
        let (indexed_files, indexed_bytes) = validate_indexed_brp_resources(candidates)?;
        Ok(Self {
            indexed_files,
            indexed_bytes,
            ..Self::default()
        })
    }

    #[cfg(test)]
    fn from_brp_index(index: &ResmedSessionIndex) -> Result<Self, ImportError> {
        let candidates: Vec<_> = index.candidates.iter().collect();
        Self::from_brp_candidates(&candidates)
    }

    fn reserve_optional_indexed_file(
        &mut self,
        file: &ResmedSessionFile,
    ) -> Result<(), ImportError> {
        if file.size_bytes > u64::try_from(RESMED_BRP_MAX_FILE_BYTES).expect("file budget fits u64")
        {
            return Err(aggregate_limit_error(format!(
                "{} file exceeds the {RESMED_BRP_MAX_FILE_BYTES}-byte import budget",
                detail_kind_name(file.kind)
            )));
        }
        let indexed_files = self.indexed_files.checked_add(1).ok_or_else(|| {
            aggregate_limit_error("ResMed detail file-count arithmetic overflowed")
        })?;
        if indexed_files > RESMED_BRP_MAX_FILES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed detail import accepts at most {RESMED_BRP_MAX_FILES_PER_IMPORT} files"
            )));
        }
        let indexed_bytes = self
            .indexed_bytes
            .checked_add(file.size_bytes)
            .ok_or_else(|| {
                aggregate_limit_error("aggregate ResMed detail byte-count arithmetic overflowed")
            })?;
        if indexed_bytes > RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed detail import accepts at most {RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT} indexed bytes"
            )));
        }
        self.indexed_files = indexed_files;
        self.indexed_bytes = indexed_bytes;
        Ok(())
    }

    fn ensure_optional_output_capacity(&self) -> Result<(), ImportError> {
        if self.output_samples >= RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT
            || self.output_series >= RESMED_DETAIL_MAX_OUTPUT_SERIES_PER_IMPORT
        {
            return Err(aggregate_limit_error(
                "ResMed optional detail output capacity is exhausted",
            ));
        }
        Ok(())
    }

    fn charge_actual_bytes(&mut self, additional: usize) -> Result<(), ImportError> {
        let additional = u64::try_from(additional).map_err(|_| {
            aggregate_limit_error(
                "actual ResMed detail byte count exceeds the supported integer range",
            )
        })?;
        let next = self
            .actual_bytes_read
            .checked_add(additional)
            .ok_or_else(|| {
                aggregate_limit_error(
                    "aggregate actual ResMed detail byte-count arithmetic overflowed",
                )
            })?;
        if next > RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed detail import reads at most {RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT} actual payload bytes"
            )));
        }
        self.actual_bytes_read = next;
        Ok(())
    }

    fn next_actual_read_limit(&self) -> Result<usize, ImportError> {
        let remaining = RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT
            .checked_sub(self.actual_bytes_read)
            .ok_or_else(|| {
                aggregate_limit_error(
                    "aggregate actual ResMed detail byte accounting is inconsistent",
                )
            })?;
        if remaining == 0 {
            return Err(aggregate_limit_error(format!(
                "ResMed detail import reads at most {RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT} actual payload bytes"
            )));
        }
        let remaining = usize::try_from(remaining).unwrap_or(usize::MAX);
        Ok(RESMED_BRP_MAX_FILE_BYTES.min(remaining))
    }

    fn reserve_output(
        &mut self,
        additional_samples: usize,
        additional_series: usize,
    ) -> Result<(), ImportError> {
        let output_samples = self
            .output_samples
            .checked_add(additional_samples)
            .ok_or_else(|| {
                aggregate_limit_error("aggregate ResMed detail output-sample arithmetic overflowed")
            })?;
        if output_samples > RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed detail import accepts at most {RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT} calibrated output samples"
            )));
        }
        let output_series = self
            .output_series
            .checked_add(additional_series)
            .ok_or_else(|| {
                aggregate_limit_error("aggregate ResMed detail output-series arithmetic overflowed")
            })?;
        if output_series > RESMED_DETAIL_MAX_OUTPUT_SERIES_PER_IMPORT {
            return Err(aggregate_limit_error(format!(
                "ResMed detail import accepts at most {RESMED_DETAIL_MAX_OUTPUT_SERIES_PER_IMPORT} output series"
            )));
        }
        self.output_samples = output_samples;
        self.output_series = output_series;
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

const fn is_decoded_detail_kind(kind: ResmedSessionFileKind) -> bool {
    matches!(
        kind,
        ResmedSessionFileKind::Brp | ResmedSessionFileKind::Sad | ResmedSessionFileKind::Sa2
    )
}

fn aggregate_limit_error(message: impl Into<String>) -> ImportError {
    ImportError::new(ImportErrorKind::SizeLimitExceeded, message)
}

fn decode_brp_candidate(
    source: &dyn ImportSource,
    candidate: &ResmedSessionCandidate,
    machine_serial: &str,
    clock: &ImportClockContext,
    statistics: &mut ImportStatistics,
    import_budget: &mut DetailImportBudget,
) -> Result<CandidateDecode, ImportError> {
    let mut warnings = Vec::new();
    let mut brp_files = Vec::new();
    let str_anchor = str_anchor(candidate)?;

    for file in candidate
        .files
        .iter()
        .filter(|file| file.kind == ResmedSessionFileKind::Brp)
    {
        match decode_brp_file(
            source,
            file,
            machine_serial,
            clock,
            statistics,
            import_budget,
        ) {
            Ok(decoded) => {
                warnings.extend(decoded.1);
                if let Some(decoded_file) = decoded.0
                    && !decoded_file.waveforms.is_empty()
                {
                    brp_files.push((file, decoded_file));
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

    if brp_files.is_empty() && str_anchor.is_none() {
        warnings.push(warning(
            "resmed_candidate_not_imported",
            "Candidate has no trustworthy decoded BRP waveform data",
            None,
            None,
        ));
        return Ok(CandidateDecode {
            session: None,
            warnings,
            brp_anchored: false,
        });
    }

    let mut brp_start_ms = i64::MAX;
    let mut brp_end_ms = i64::MIN;
    let mut brp_intervals = Vec::with_capacity(brp_files.len());
    for (_, file) in &brp_files {
        brp_start_ms = brp_start_ms.min(file.local_start_ms);
        brp_end_ms = brp_end_ms.max(file.local_end_ms);
        brp_intervals.push((file.local_start_ms, file.local_end_ms));
    }

    if !brp_files.is_empty() && brp_end_ms <= brp_start_ms {
        warnings.push(warning(
            "resmed_brp_not_decoded",
            "Candidate BRP detail had no positive calibrated time envelope and was ignored",
            None,
            None,
        ));
        brp_files.clear();
        brp_intervals.clear();
        brp_start_ms = i64::MAX;
        brp_end_ms = i64::MIN;
    }

    if brp_files.is_empty() && str_anchor.is_none() {
        warnings.push(warning(
            "resmed_candidate_not_imported",
            "Candidate had no supported, calibrated BRP signals and was not emitted as a session",
            None,
            None,
        ));
        return Ok(CandidateDecode {
            session: None,
            warnings,
            brp_anchored: false,
        });
    }

    let brp_anchored = !brp_files.is_empty();
    let (source_key, usage_ms, envelope_start_ms, envelope_end_ms, therapy_day, slices) =
        if let Some(anchor) = &str_anchor {
            let raw_mask_on_minute = u16::try_from(anchor.raw_mask_on_value).map_err(|_| {
                ImportError::new(
                    ImportErrorKind::InvalidData,
                    "STR source mask-on value is not a nonnegative raw u16 minute",
                )
            })?;
            let mask_on_bytes = raw_mask_on_minute.to_be_bytes();
            let source_key = opaque_key(
                "opap/resmed/str-mask-session-source/v1",
                [anchor.therapy_day.as_bytes(), mask_on_bytes.as_slice()],
            );
            let usage_ms = anchor
                .end_ms
                .checked_sub(anchor.start_ms)
                .and_then(|duration| u64::try_from(duration).ok())
                .ok_or_else(|| {
                    ImportError::new(
                        ImportErrorKind::InvalidData,
                        "STR mask interval has no positive bounded duration",
                    )
                })?;
            let envelope_start_ms = if brp_anchored {
                anchor.start_ms.min(brp_start_ms)
            } else {
                anchor.start_ms
            };
            let envelope_end_ms = if brp_anchored {
                anchor.end_ms.max(brp_end_ms)
            } else {
                anchor.end_ms
            };
            let slice_start = session_timestamp(anchor.start_ms, clock)?;
            let slice_end = session_timestamp(anchor.end_ms, clock)?;
            let slice_source_key = opaque_key(
                "opap/resmed/str-mask-slice-source/v1",
                std::iter::once(source_key.as_bytes()),
            );
            (
                source_key,
                usage_ms,
                envelope_start_ms,
                envelope_end_ms,
                anchor.therapy_day.clone(),
                vec![TherapySlice {
                    source_key: slice_source_key,
                    state: TherapySliceState::MaskOn,
                    start_time_unix_ms: slice_start.normalized_utc_unix_ms,
                    end_time_unix_ms: slice_end.normalized_utc_unix_ms,
                }],
            )
        } else {
            let usage_ms = covered_duration_millis(&mut brp_intervals)?;
            (
                logical_brp_session_source_key(candidate, &brp_files, brp_start_ms),
                usage_ms,
                brp_start_ms,
                brp_end_ms,
                candidate.resmed_day.clone(),
                Vec::new(),
            )
        };
    let id = opaque_key(
        "opap/resmed/session-id/v2",
        std::iter::once(source_key.as_bytes()),
    );

    let mut waveforms = Vec::new();
    let mut channels = BTreeMap::<String, ChannelMetadata>::new();
    for (_, file) in brp_files {
        for channel in file.channels {
            channels.entry(channel.id.clone()).or_insert(channel);
        }
        waveforms.extend(file.waveforms);
    }

    let start_time = session_timestamp(envelope_start_ms, clock)?;
    let end_time = session_timestamp(envelope_end_ms, clock)?;
    let session_duration_ms = end_time
        .normalized_utc_unix_ms
        .checked_sub(start_time.normalized_utc_unix_ms)
        .and_then(|duration| u64::try_from(duration).ok())
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidConfiguration,
                "clock context produced an invalid session duration",
            )
        })?;
    if usage_ms > session_duration_ms {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "BRP coverage exceeds the decoded ResMed session envelope",
        ));
    }

    for warning in &mut warnings {
        warning.session_id = Some(id.clone());
    }

    Ok(CandidateDecode {
        session: Some(Session {
            id,
            source_key,
            therapy_day,
            data_kind: if brp_anchored {
                SessionDataKind::Partial
            } else {
                SessionDataKind::SummaryOnly
            },
            start_time,
            end_time,
            slices,
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
        brp_anchored,
    })
}

struct StrMaskAnchor {
    therapy_day: String,
    raw_mask_on_value: i16,
    start_ms: i64,
    end_ms: i64,
}

fn str_anchor(candidate: &ResmedSessionCandidate) -> Result<Option<StrMaskAnchor>, ImportError> {
    let ResmedSessionAnchor::StrMask {
        therapy_day,
        mask_on_minute,
        mask_off_minute,
        source_mask_on_value,
        source_mask_off_value,
        start_time,
        end_time,
        repair,
    } = &candidate.anchor
    else {
        return Ok(None);
    };
    let raw_mask_on_value =
        source_mask_on_value.unwrap_or_else(|| i16::try_from(*mask_on_minute).unwrap_or(i16::MIN));
    let raw_mask_off_value = source_mask_off_value.unwrap_or_else(|| {
        if *repair == Some(StrBoundaryRepair::HistoricalTrailingNoon) {
            0
        } else {
            i16::try_from(*mask_off_minute).unwrap_or(i16::MIN)
        }
    });
    if therapy_day != &candidate.resmed_day
        || mask_off_minute <= mask_on_minute
        || i16::try_from(*mask_on_minute).ok() != Some(raw_mask_on_value)
    {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "STR candidate anchor is inconsistent with its therapy day or selected mask interval",
        ));
    }
    let valid_source_end = match repair {
        None => i16::try_from(*mask_off_minute).ok() == Some(raw_mask_off_value),
        Some(StrBoundaryRepair::SlotZeroContinuation) => {
            raw_mask_on_value == 0
                && *mask_on_minute == 0
                && i16::try_from(*mask_off_minute).ok() == Some(raw_mask_off_value)
        }
        Some(StrBoundaryRepair::HistoricalTrailingNoon) => {
            raw_mask_on_value > 0 && raw_mask_off_value <= 0 && *mask_off_minute == 1_440
        }
    };
    if !valid_source_end {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "STR candidate selected boundary is inconsistent with its source values and repair",
        ));
    }
    let start_ms =
        local_unix_millis(&device_local_from_resmed(start_time)?).map_err(invalid_source_clock)?;
    let end_ms =
        local_unix_millis(&device_local_from_resmed(end_time)?).map_err(invalid_source_clock)?;
    let expected_duration_ms = i64::from(mask_off_minute - mask_on_minute).saturating_mul(60_000);
    if end_ms.checked_sub(start_ms) != Some(expected_duration_ms) {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "STR candidate selected times do not match its selected minute offsets",
        ));
    }
    Ok(Some(StrMaskAnchor {
        therapy_day: therapy_day.clone(),
        raw_mask_on_value,
        start_ms,
        end_ms,
    }))
}

#[allow(clippy::too_many_arguments)]
fn attach_candidate_oximetry(
    source: &dyn ImportSource,
    candidate: &ResmedSessionCandidate,
    machine_serial: &str,
    clock: &ImportClockContext,
    session: &mut Session,
    warnings: &mut Vec<ImportWarning>,
    statistics: &mut ImportStatistics,
    import_budget: &mut DetailImportBudget,
) -> Result<(), ImportError> {
    let mut local_start_ms = session
        .start_time
        .device_local
        .as_ref()
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidData,
                "BRP-backed session is missing structured local start provenance",
            )
        })
        .and_then(|value| local_unix_millis(value).map_err(invalid_source_clock))?;
    let mut local_end_ms = session
        .end_time
        .device_local
        .as_ref()
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidData,
                "BRP-backed session is missing structured local end provenance",
            )
        })
        .and_then(|value| local_unix_millis(value).map_err(invalid_source_clock))?;
    let mut channels: BTreeMap<_, _> = std::mem::take(&mut session.channels)
        .into_iter()
        .map(|channel| (channel.id.clone(), channel))
        .collect();

    for file in candidate.files.iter().filter(|file| {
        matches!(
            file.kind,
            ResmedSessionFileKind::Sad | ResmedSessionFileKind::Sa2
        )
    }) {
        if let Err(error) = import_budget.ensure_optional_output_capacity() {
            warnings.push(optional_oximetry_warning(file, &session.id, &error));
            continue;
        }
        if let Err(error) = import_budget.reserve_optional_indexed_file(file) {
            warnings.push(optional_oximetry_warning(file, &session.id, &error));
            continue;
        }
        match decode_oximetry_file(
            source,
            file,
            machine_serial,
            clock,
            statistics,
            import_budget,
        ) {
            Ok(decoded) => {
                warnings.extend(decoded.1);
                let Some(file) = decoded.0 else {
                    continue;
                };
                if file.waveforms.is_empty() {
                    continue;
                }
                // Storage requires each waveform to lie within its parent
                // session. Expand only the session envelope; BRP coverage
                // remains the sole source of therapy usage in this partial
                // importer.
                local_start_ms = local_start_ms.min(file.local_start_ms);
                local_end_ms = local_end_ms.max(file.local_end_ms);
                for channel in file.channels {
                    channels.entry(channel.id.clone()).or_insert(channel);
                }
                session.waveforms.extend(file.waveforms);
            }
            Err(error) => warnings.push(optional_oximetry_warning(file, &session.id, &error)),
        }
    }

    session.channels = channels.into_values().collect();
    if local_end_ms <= local_start_ms {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "decoded ResMed session envelope is empty or reversed",
        ));
    }
    session.start_time = session_timestamp(local_start_ms, clock)?;
    session.end_time = session_timestamp(local_end_ms, clock)?;
    let session_duration_ms = session
        .end_time
        .normalized_utc_unix_ms
        .checked_sub(session.start_time.normalized_utc_unix_ms)
        .and_then(|duration| u64::try_from(duration).ok())
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidConfiguration,
                "clock context produced an invalid session duration",
            )
        })?;
    if session.summary.usage_ms > session_duration_ms {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "BRP coverage exceeds the decoded ResMed session envelope",
        ));
    }
    for warning in warnings {
        warning.session_id = Some(session.id.clone());
    }
    Ok(())
}

fn optional_oximetry_warning(
    file: &ResmedSessionFile,
    session_id: &str,
    error: &ImportError,
) -> ImportWarning {
    warning(
        format!("resmed_{}_not_decoded", detail_kind_lowercase(file.kind)),
        format!(
            "{} oximetry was skipped: {error}",
            detail_kind_name(file.kind)
        ),
        Some(&file.relative_path),
        Some(session_id),
    )
}

fn decode_brp_file(
    source: &dyn ImportSource,
    indexed_file: &ResmedSessionFile,
    machine_serial: &str,
    clock: &ImportClockContext,
    statistics: &mut ImportStatistics,
    import_budget: &mut DetailImportBudget,
) -> Result<(Option<DecodedDetailFile>, Vec<ImportWarning>), ImportError> {
    let (detail, mut warnings) = read_detail_edf(
        source,
        indexed_file,
        machine_serial,
        statistics,
        import_budget,
    )?;
    let Some(detail) = detail else {
        return Ok((None, warnings));
    };

    let source_discriminator = source_basename_discriminator(indexed_file);
    let mut waveforms = Vec::new();
    let mut channels = Vec::new();
    for (signal_index, signal) in detail.parsed.signals().iter().enumerate() {
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
        let Some((sample_interval_ms, source_encoding)) =
            validated_signal_encoding(&detail.parsed, signal, label, indexed_file, &mut warnings)
        else {
            continue;
        };

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
        let mut valid = true;
        for value in physical.clone() {
            let normalized = value * flow_scale;
            #[allow(clippy::cast_possible_truncation)]
            let normalized = normalized as f32;
            if !normalized.is_finite() {
                valid = false;
                break;
            }
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
        import_budget.reserve_output(physical.len(), 1)?;
        let samples = physical
            .map(|value| {
                #[allow(clippy::cast_possible_truncation)]
                {
                    (value * flow_scale) as f32
                }
            })
            .collect();

        let signal_index_bytes = u64::try_from(signal_index)
            .unwrap_or(u64::MAX)
            .to_be_bytes();
        let source_key = opaque_key(
            "opap/resmed/brp-waveform/v1",
            [
                detail.fingerprint.as_slice(),
                source_discriminator.as_slice(),
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
            start_time_unix_ms: normalize_millis(detail.local_start_ms, clock)?,
            sample_interval_ms,
            samples,
            source_encoding: Some(source_encoding),
        });
    }

    Ok((
        Some(DecodedDetailFile {
            local_start_ms: detail.local_start_ms,
            local_end_ms: detail.local_end_ms,
            waveforms,
            channels,
        }),
        warnings,
    ))
}

fn decode_oximetry_file(
    source: &dyn ImportSource,
    indexed_file: &ResmedSessionFile,
    machine_serial: &str,
    clock: &ImportClockContext,
    statistics: &mut ImportStatistics,
    import_budget: &mut DetailImportBudget,
) -> Result<(Option<DecodedDetailFile>, Vec<ImportWarning>), ImportError> {
    let registry_kind = match indexed_file.kind {
        ResmedSessionFileKind::Sad => ResmedFileKind::Sad,
        ResmedSessionFileKind::Sa2 => ResmedFileKind::Sa2,
        _ => {
            return Err(ImportError::new(
                ImportErrorKind::InvalidData,
                "non-oximetry file passed to the SAD/SA2 decoder",
            )
            .at_path(&indexed_file.relative_path));
        }
    };
    let (detail, mut warnings) = read_detail_edf(
        source,
        indexed_file,
        machine_serial,
        statistics,
        import_budget,
    )?;
    let Some(detail) = detail else {
        return Ok((None, warnings));
    };

    let source_discriminator = source_basename_discriminator(indexed_file);
    let normalized_file_start = normalize_millis(detail.local_start_ms, clock)?;
    let mut waveforms = Vec::new();
    let mut channels = Vec::new();
    for (signal_index, signal) in detail.parsed.signals().iter().enumerate() {
        let label = signal.header.label.trim();
        if label.eq_ignore_ascii_case("Crc16") {
            continue;
        }
        let Some(channel) = resmed_signal_prefix(registry_kind, label) else {
            warnings.push(warning(
                format!(
                    "unknown_resmed_{}_signal",
                    detail_kind_lowercase(indexed_file.kind)
                ),
                format!(
                    "Unknown {} oximetry signal ignored: {label}",
                    detail_kind_name(indexed_file.kind)
                ),
                Some(&indexed_file.relative_path),
                None,
            ));
            continue;
        };
        if !matches!(
            channel.key.as_str(),
            PULSE_RATE_CHANNEL | OXYGEN_SATURATION_CHANNEL
        ) {
            warnings.push(warning(
                format!(
                    "invalid_resmed_{}_signal",
                    detail_kind_lowercase(indexed_file.kind)
                ),
                format!(
                    "{} signal {label} resolved outside the bounded oximetry channel set",
                    detail_kind_name(indexed_file.kind)
                ),
                Some(&indexed_file.relative_path),
                None,
            ));
            continue;
        }
        let Some((sample_interval_ms, source_encoding)) =
            validated_signal_encoding(&detail.parsed, signal, label, indexed_file, &mut warnings)
        else {
            continue;
        };
        let Some(digital_samples) = signal.digital_samples() else {
            warnings.push(warning(
                format!(
                    "invalid_resmed_{}_signal",
                    detail_kind_lowercase(indexed_file.kind)
                ),
                format!(
                    "{} signal {label} is not a digital sampled signal",
                    detail_kind_name(indexed_file.kind)
                ),
                Some(&indexed_file.relative_path),
                None,
            ));
            continue;
        };
        let physical = match signal.physical_samples() {
            Ok(physical) => physical,
            Err(error) => {
                warnings.push(warning(
                    format!(
                        "invalid_resmed_{}_calibration",
                        detail_kind_lowercase(indexed_file.kind)
                    ),
                    format!(
                        "{} signal {label} has invalid EDF calibration: {error}",
                        detail_kind_name(indexed_file.kind)
                    ),
                    Some(&indexed_file.relative_path),
                    None,
                ));
                continue;
            }
        };

        let mut output_samples = 0usize;
        let mut output_segments = 0usize;
        let mut in_segment = false;
        let mut calibration_is_valid = true;
        for (&digital, physical) in digital_samples.iter().zip(physical) {
            if digital == -1 {
                in_segment = false;
                continue;
            }
            if !in_segment {
                output_segments = output_segments.checked_add(1).ok_or_else(|| {
                    aggregate_limit_error("oximetry output-segment arithmetic overflowed")
                })?;
                in_segment = true;
            }
            #[allow(clippy::cast_possible_truncation)]
            let physical = physical as f32;
            if !physical.is_finite() {
                calibration_is_valid = false;
                break;
            }
            output_samples = output_samples.checked_add(1).ok_or_else(|| {
                aggregate_limit_error("oximetry output-sample arithmetic overflowed")
            })?;
        }
        if !calibration_is_valid {
            warnings.push(warning(
                format!(
                    "invalid_resmed_{}_calibration",
                    detail_kind_lowercase(indexed_file.kind)
                ),
                format!(
                    "{} signal {label} calibration produced a non-finite sample",
                    detail_kind_name(indexed_file.kind)
                ),
                Some(&indexed_file.relative_path),
                None,
            ));
            continue;
        }
        if output_samples == 0 {
            continue;
        }
        import_budget.reserve_output(output_samples, output_segments)?;

        let signal_index_bytes = u64::try_from(signal_index)
            .unwrap_or(u64::MAX)
            .to_be_bytes();
        let mut segment_start_index = None;
        let mut segment_samples = Vec::new();
        let mut segment_ordinal = 0usize;
        for (sample_index, (&digital, physical)) in digital_samples
            .iter()
            .zip(
                signal
                    .physical_samples()
                    .expect("calibration was validated above"),
            )
            .enumerate()
        {
            if digital == -1 {
                if let Some(start_index) = segment_start_index.take() {
                    push_oximetry_segment(
                        &mut waveforms,
                        &mut segment_samples,
                        indexed_file.kind,
                        detail.fingerprint,
                        source_discriminator,
                        signal_index_bytes,
                        segment_ordinal,
                        start_index,
                        normalized_file_start,
                        sample_interval_ms,
                        channel.key.as_str(),
                        source_encoding,
                    )?;
                    segment_ordinal = segment_ordinal.saturating_add(1);
                }
                continue;
            }
            if segment_start_index.is_none() {
                segment_start_index = Some(sample_index);
            }
            #[allow(clippy::cast_possible_truncation)]
            segment_samples.push(physical as f32);
        }
        if let Some(start_index) = segment_start_index {
            push_oximetry_segment(
                &mut waveforms,
                &mut segment_samples,
                indexed_file.kind,
                detail.fingerprint,
                source_discriminator,
                signal_index_bytes,
                segment_ordinal,
                start_index,
                normalized_file_start,
                sample_interval_ms,
                channel.key.as_str(),
                source_encoding,
            )?;
        }

        let unit = channel.unit.symbol();
        channels.push(ChannelMetadata {
            id: channel.key.as_str().to_owned(),
            label: channel.label.to_owned(),
            unit: (!unit.is_empty()).then(|| unit.to_owned()),
            kind: ChannelKind::Waveform,
        });
    }

    Ok((
        Some(DecodedDetailFile {
            local_start_ms: detail.local_start_ms,
            local_end_ms: detail.local_end_ms,
            waveforms,
            channels,
        }),
        warnings,
    ))
}

#[allow(clippy::too_many_arguments)]
fn push_oximetry_segment(
    waveforms: &mut Vec<WaveformSeries>,
    samples: &mut Vec<f32>,
    file_kind: ResmedSessionFileKind,
    fingerprint: [u8; 32],
    source_discriminator: [u8; 32],
    signal_index_bytes: [u8; 8],
    segment_ordinal: usize,
    start_sample_index: usize,
    normalized_file_start: i64,
    sample_interval_ms: f64,
    channel_id: &str,
    source_encoding: EdfSourceEncoding,
) -> Result<(), ImportError> {
    if samples.is_empty() {
        return Ok(());
    }
    let segment_ordinal_bytes = u64::try_from(segment_ordinal)
        .unwrap_or(u64::MAX)
        .to_be_bytes();
    let start_sample_bytes = u64::try_from(start_sample_index)
        .unwrap_or(u64::MAX)
        .to_be_bytes();
    let source_key = opaque_key(
        oximetry_segment_key_domain(file_kind),
        [
            fingerprint.as_slice(),
            source_discriminator.as_slice(),
            signal_index_bytes.as_slice(),
            segment_ordinal_bytes.as_slice(),
            start_sample_bytes.as_slice(),
            channel_id.as_bytes(),
        ],
    );
    let start_offset_ms = sample_offset_millis(start_sample_index, sample_interval_ms)?;
    let start_time_unix_ms = normalized_file_start
        .checked_add(start_offset_ms)
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidData,
                "oximetry segment start exceeds the supported timestamp range",
            )
        })?;
    waveforms.push(WaveformSeries {
        source_key,
        channel_id: channel_id.to_owned(),
        start_time_unix_ms,
        sample_interval_ms,
        samples: std::mem::take(samples),
        source_encoding: Some(source_encoding),
    });
    Ok(())
}

fn sample_offset_millis(sample_index: usize, sample_interval_ms: f64) -> Result<i64, ImportError> {
    let sample_index = u32::try_from(sample_index).map_err(|_| {
        ImportError::new(
            ImportErrorKind::SizeLimitExceeded,
            "oximetry sample index exceeds the supported range",
        )
    })?;
    let offset = f64::from(sample_index) * sample_interval_ms;
    if !offset.is_finite() || offset < 0.0 || offset > i64::MAX as f64 {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "oximetry sample cadence exceeds the supported timestamp range",
        ));
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(offset.round() as i64)
}

fn read_detail_edf(
    source: &dyn ImportSource,
    indexed_file: &ResmedSessionFile,
    machine_serial: &str,
    statistics: &mut ImportStatistics,
    import_budget: &mut DetailImportBudget,
) -> Result<(Option<ParsedDetailEdf>, Vec<ImportWarning>), ImportError> {
    let kind_name = detail_kind_name(indexed_file.kind);
    let kind_lowercase = detail_kind_lowercase(indexed_file.kind);
    if !is_decoded_detail_kind(indexed_file.kind) {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "unsupported file kind passed to the ResMed detail decoder",
        )
        .at_path(&indexed_file.relative_path));
    }
    if indexed_file
        .relative_path
        .to_ascii_lowercase()
        .ends_with(".gz")
    {
        return Err(ImportError::new(
            ImportErrorKind::UnsupportedOperation,
            format!("compressed {kind_name} payloads are not supported"),
        )
        .at_path(&indexed_file.relative_path));
    }
    if indexed_file.size_bytes
        > u64::try_from(RESMED_BRP_MAX_FILE_BYTES).expect("file budget fits u64")
    {
        return Err(ImportError::new(
            ImportErrorKind::SizeLimitExceeded,
            format!("{kind_name} file exceeds the {RESMED_BRP_MAX_FILE_BYTES}-byte import budget"),
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
                "source adapter returned {} bytes for a {kind_name} file, exceeding the {}-byte import budget",
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
            format!("{kind_name} file size changed after candidate indexing"),
        )
        .at_path(&indexed_file.relative_path));
    }

    let parsed = Parser::new(BRP_LIMITS).parse(&bytes).map_err(|source| {
        ImportError::new(
            ImportErrorKind::InvalidData,
            format!("failed to decode complete {kind_name} EDF: {source}"),
        )
        .at_path(&indexed_file.relative_path)
    })?;
    let summary = indexed_file.edf_header.as_ref().ok_or_else(|| {
        ImportError::new(
            ImportErrorKind::InvalidData,
            format!("candidate {kind_name} file has no indexed EDF header summary"),
        )
        .at_path(&indexed_file.relative_path)
    })?;
    validate_header_summary(parsed.header(), summary, kind_name).map_err(|message| {
        ImportError::new(ImportErrorKind::InvalidData, message).at_path(&indexed_file.relative_path)
    })?;
    if parsed.header().is_discontinuous() {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            format!("EDF+D {kind_name} files are discontinuous and are not supported"),
        )
        .at_path(&indexed_file.relative_path));
    }

    let mut warnings = Vec::new();
    match recording_serial(&parsed.header().recording_id) {
        Some(serial) if serial != machine_serial => {
            warnings.push(warning(
                format!("resmed_{kind_lowercase}_serial_mismatch"),
                format!(
                    "{kind_name} machine identity did not match the selected card; file skipped"
                ),
                Some(&indexed_file.relative_path),
                None,
            ));
            return Ok((None, warnings));
        }
        Some(_) => {}
        None => warnings.push(warning(
            format!("resmed_{kind_lowercase}_serial_missing"),
            format!(
                "{kind_name} recording header has no SRN token; file imported without per-file identity verification"
            ),
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
            format!("decoded {kind_name} duration is zero, non-finite, or outside timestamp range"),
        )
        .at_path(&indexed_file.relative_path)
    })?;
    let local_end_ms = local_start_ms.checked_add(duration_ms).ok_or_else(|| {
        ImportError::new(
            ImportErrorKind::InvalidData,
            format!("decoded {kind_name} end time exceeds the supported calendar range"),
        )
        .at_path(&indexed_file.relative_path)
    })?;
    local_datetime_from_millis(local_end_ms).map_err(invalid_source_clock)?;

    Ok((
        Some(ParsedDetailEdf {
            parsed,
            local_start_ms,
            local_end_ms,
            fingerprint: sha256(&bytes),
        }),
        warnings,
    ))
}

fn validated_signal_encoding(
    parsed: &EdfFile,
    signal: &Signal,
    label: &str,
    indexed_file: &ResmedSessionFile,
    warnings: &mut Vec<ImportWarning>,
) -> Option<(f64, EdfSourceEncoding)> {
    let kind_name = detail_kind_name(indexed_file.kind);
    let kind_lowercase = detail_kind_lowercase(indexed_file.kind);
    if signal.header.samples_per_record == 0 {
        warnings.push(warning(
            format!("invalid_resmed_{kind_lowercase}_signal"),
            format!("{kind_name} signal {label} has zero samples per record"),
            Some(&indexed_file.relative_path),
            None,
        ));
        return None;
    }
    let Ok(samples_per_record) = u32::try_from(signal.header.samples_per_record) else {
        warnings.push(warning(
            format!("invalid_resmed_{kind_lowercase}_signal"),
            format!("{kind_name} signal {label} sample cadence exceeds the supported range"),
            Some(&indexed_file.relative_path),
            None,
        ));
        return None;
    };
    let sample_interval_ms =
        parsed.header().record_duration_seconds * 1_000.0 / f64::from(samples_per_record);
    if !sample_interval_ms.is_finite() || sample_interval_ms <= 0.0 {
        warnings.push(warning(
            format!("invalid_resmed_{kind_lowercase}_signal"),
            format!("{kind_name} signal {label} has an invalid sampling interval"),
            Some(&indexed_file.relative_path),
            None,
        ));
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

fn detail_kind_name(kind: ResmedSessionFileKind) -> &'static str {
    match kind {
        ResmedSessionFileKind::Brp => "BRP",
        ResmedSessionFileKind::Sad => "SAD",
        ResmedSessionFileKind::Sa2 => "SA2",
        _ => "ResMed detail",
    }
}

fn detail_kind_lowercase(kind: ResmedSessionFileKind) -> &'static str {
    match kind {
        ResmedSessionFileKind::Brp => "brp",
        ResmedSessionFileKind::Sad => "sad",
        ResmedSessionFileKind::Sa2 => "sa2",
        _ => "detail",
    }
}

fn oximetry_segment_key_domain(kind: ResmedSessionFileKind) -> &'static str {
    match kind {
        ResmedSessionFileKind::Sad => "opap/resmed/sad-waveform-segment/v1",
        ResmedSessionFileKind::Sa2 => "opap/resmed/sa2-waveform-segment/v1",
        _ => "opap/resmed/invalid-oximetry-waveform-segment/v1",
    }
}

fn source_basename_discriminator(file: &ResmedSessionFile) -> [u8; 32] {
    let basename = file
        .relative_path
        .rsplit('/')
        .next()
        .unwrap_or(file.relative_path.as_str());
    sha256(basename.as_bytes())
}

fn covered_duration_millis(intervals: &mut [(i64, i64)]) -> Result<u64, ImportError> {
    intervals.sort_unstable();
    let Some(&(mut current_start, mut current_end)) = intervals.first() else {
        return Ok(0);
    };
    if current_end <= current_start {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "decoded BRP coverage contains an empty or reversed interval",
        ));
    }
    let mut total = 0u64;
    for &(start, end) in &intervals[1..] {
        if end <= start {
            return Err(ImportError::new(
                ImportErrorKind::InvalidData,
                "decoded BRP coverage contains an empty or reversed interval",
            ));
        }
        if start <= current_end {
            current_end = current_end.max(end);
            continue;
        }
        total = total
            .checked_add(u64::try_from(current_end - current_start).map_err(|_| {
                ImportError::new(
                    ImportErrorKind::InvalidData,
                    "decoded BRP coverage duration is outside the supported range",
                )
            })?)
            .ok_or_else(|| {
                ImportError::new(
                    ImportErrorKind::InvalidData,
                    "decoded BRP coverage duration overflowed",
                )
            })?;
        current_start = start;
        current_end = end;
    }
    total
        .checked_add(u64::try_from(current_end - current_start).map_err(|_| {
            ImportError::new(
                ImportErrorKind::InvalidData,
                "decoded BRP coverage duration is outside the supported range",
            )
        })?)
        .ok_or_else(|| {
            ImportError::new(
                ImportErrorKind::InvalidData,
                "decoded BRP coverage duration overflowed",
            )
        })
}

/// Logical identity for BRP-backed detail-fallback sessions.
///
/// Payload fingerprints deliberately stay out of this key so authoritative
/// replacement can update changed children without duplicating the logical
/// session. Serial-verified STR candidates use their separate therapy-day plus
/// raw mask-on identity domain instead.
fn logical_brp_session_source_key(
    candidate: &ResmedSessionCandidate,
    brp_files: &[(&ResmedSessionFile, DecodedDetailFile)],
    anchor_start_ms: i64,
) -> String {
    let anchor_start_bytes = anchor_start_ms.to_be_bytes();
    let anchor_basename = brp_files
        .iter()
        .filter(|(_, decoded)| decoded.local_start_ms == anchor_start_ms)
        .filter_map(|(file, _)| file.relative_path.rsplit('/').next())
        .min()
        .expect("decoded BRP anchor has a source basename");
    opaque_key(
        "opap/resmed/brp-session-source/v2",
        [
            candidate.resmed_day.as_bytes(),
            anchor_start_bytes.as_slice(),
            anchor_basename.as_bytes(),
        ],
    )
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
    kind_name: &str,
) -> Result<(), String> {
    if u64::try_from(header.header_bytes).ok() != Some(summary.header_bytes)
        || u16::try_from(header.signals.len()).ok() != Some(summary.signal_count)
        || header
            .declared_record_count
            .and_then(|count| u64::try_from(count).ok())
            != summary.declared_record_count
        || header.record_duration_seconds.to_bits() != summary.record_duration_seconds.to_bits()
    {
        return Err(format!(
            "complete {kind_name} header no longer matches its indexed summary"
        ));
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
            return Err(format!(
                "complete {kind_name} start time no longer matches its indexed summary"
            ));
        }
    }
    Ok(())
}

fn candidate_ends_at_or_before(
    candidate: &ResmedSessionCandidate,
    cutoff: i64,
    clock: &ImportClockContext,
) -> bool {
    let mut saw_brp = false;
    let mut latest_upper_bound = i64::MIN;
    let mut has_authoritative_str_anchor = false;
    if let ResmedSessionAnchor::StrMask { end_time, .. } = &candidate.anchor {
        let Ok(device_local) = device_local_from_resmed(end_time) else {
            return false;
        };
        let Ok(local_end_ms) = local_unix_millis(&device_local) else {
            return false;
        };
        latest_upper_bound = local_end_ms;
        has_authoritative_str_anchor = true;
    }
    for file in candidate
        .files
        .iter()
        .filter(|file| is_decoded_detail_kind(file.kind))
    {
        saw_brp |= file.kind == ResmedSessionFileKind::Brp;
        let Some(summary) = &file.edf_header else {
            return false;
        };
        let Some(record_count) = summary.declared_record_count else {
            // The full parser can infer records from payload bytes. A
            // point-attached SAD/SA2 therefore has no proven indexed end.
            return false;
        };
        let Ok(record_count) = u32::try_from(record_count) else {
            return false;
        };
        let duration_ms = f64::from(record_count) * summary.record_duration_seconds * 1_000.0;
        if !duration_ms.is_finite() || duration_ms <= 0.0 || duration_ms > i64::MAX as f64 {
            return false;
        }
        #[allow(clippy::cast_possible_truncation)]
        let duration_upper_bound_ms = duration_ms.ceil() as i64;
        let Ok(device_local) = device_local_from_resmed(&file.selected_start_time) else {
            return false;
        };
        let Ok(local_start_ms) = local_unix_millis(&device_local) else {
            return false;
        };
        let Some(local_end_ms) = local_start_ms.checked_add(duration_upper_bound_ms) else {
            return false;
        };
        latest_upper_bound = latest_upper_bound.max(local_end_ms);
    }
    // Any mutation to the complete start/count/duration header fields is
    // rejected by `validate_header_summary` if decoded. This ceiling is
    // therefore a conservative upper bound for every otherwise-accepted
    // decoded detail payload.
    (saw_brp || has_authoritative_str_anchor)
        && latest_upper_bound != i64::MIN
        && normalize_millis(latest_upper_bound, clock).is_ok_and(|end| end <= cutoff)
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
    if clock
        .timezone_basis
        .as_ref()
        .is_some_and(|basis| basis.len() > MAX_TIMEZONE_BASIS_BYTES)
    {
        return Err(ImportError::new(
            ImportErrorKind::InvalidConfiguration,
            format!("timezone_basis must be at most {MAX_TIMEZONE_BASIS_BYTES} UTF-8 bytes"),
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

        fn insert_directory(&mut self, path: &str) {
            self.inventory.entries.push(SourceEntry {
                relative_path: path.to_owned(),
                kind: SourceEntryKind::Directory,
                size_bytes: 0,
            });
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

    struct LimitIgnoringSource {
        bytes: Vec<u8>,
    }

    impl ImportSource for LimitIgnoringSource {
        fn inventory(&self) -> Result<SourceInventory, ImportError> {
            Ok(SourceInventory::default())
        }

        fn read_file(
            &self,
            _relative_path: &str,
            _max_bytes: usize,
        ) -> Result<Vec<u8>, ImportError> {
            Ok(self.bytes.clone())
        }

        fn read_file_prefix(
            &self,
            _relative_path: &str,
            max_bytes: usize,
        ) -> Result<Vec<u8>, ImportError> {
            Ok(self.bytes[..self.bytes.len().min(max_bytes)].to_vec())
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
        synthetic_detail_with_start(
            signals,
            record_count,
            record_duration,
            recording_id,
            "02.01.2622.00.00",
        )
    }

    fn synthetic_detail_with_start(
        signals: &[SignalFixture<'_>],
        record_count: usize,
        record_duration: &str,
        recording_id: &str,
        start: &str,
    ) -> Vec<u8> {
        assert_eq!(start.len(), 16);
        let header_bytes = 256 + signals.len() * 256;
        let mut bytes = Vec::new();
        bytes.extend(field("0", 8));
        bytes.extend(field("patient", 80));
        bytes.extend(field(recording_id, 80));
        bytes.extend_from_slice(start.as_bytes());
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

    fn synthetic_str(serial: &str, mask_on: &[i16], mask_off: &[i16]) -> Vec<u8> {
        assert_eq!(mask_on.len(), mask_off.len());
        assert!(!mask_on.is_empty());
        let event_count = i16::try_from(mask_on.len() * 2).expect("fixture event count");
        synthetic_detail_with_start(
            &[
                SignalFixture::new("Mask On", mask_on.len(), mask_on),
                SignalFixture::new("Mask Off", mask_off.len(), mask_off),
                SignalFixture::new("Mask Events", 1, &[event_count]),
            ],
            1,
            "86400",
            &format!("ResMed SRN={serial}"),
            "02.01.2612.00.00",
        )
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

    fn card_with_valid_str(
        mask_on: &[i16],
        mask_off: &[i16],
        files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
    ) -> MemorySource {
        card_with_str_serial("serial-123", mask_on, mask_off, files)
    }

    fn card_with_str_serial(
        str_serial: &str,
        mask_on: &[i16],
        mask_off: &[i16],
        files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
    ) -> MemorySource {
        let mut source = MemorySource::default();
        source.insert("STR.edf", synthetic_str(str_serial, mask_on, mask_off));
        source.insert(
            "Identification.tgt",
            b"#SRN serial-123\n#PNA AirSense_10_AutoSet\n#PCD 37028\n".to_vec(),
        );
        source.insert_directory("DATALOG");
        for (path, bytes) in files {
            source.insert(path, bytes);
        }
        source
    }

    fn card_with_detail_files(
        files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
    ) -> MemorySource {
        let mut source = MemorySource::default();
        source.insert("STR.edf", b"signature only".to_vec());
        source.insert(
            "Identification.tgt",
            b"#SRN serial-123\n#PNA AirSense_10_AutoSet\n#PCD 37028\n".to_vec(),
        );
        for (path, bytes) in files {
            source.insert(path, bytes);
        }
        source
    }

    fn simple_oximetry() -> Vec<u8> {
        synthetic_brp(
            &[
                SignalFixture::new("Pulse.1s", 2, &[0, 50, 100, 25]).calibration(40, 140, 0, 100),
                SignalFixture::new("SpO2", 1, &[0, 10]).calibration(80, 100, 0, 10),
            ],
            2,
            "1",
        )
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
                anchor: ResmedSessionAnchor::DetailFallback,
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
    fn imports_str_only_as_exact_summary_session_and_honors_exclusive_cutoff() {
        let source = card_with_valid_str(&[600], &[660], []);
        let report = import_resmed_sessions(&source, &options()).expect("import STR-only card");
        assert_eq!(report.sessions.len(), 1);
        let session = &report.sessions[0];
        assert_eq!(session.data_kind, SessionDataKind::SummaryOnly);
        assert_eq!(session.therapy_day, "2026-01-02");
        assert!(session.channels.is_empty());
        assert!(session.waveforms.is_empty());
        assert_eq!(session.summary.usage_ms, 60 * 60 * 1_000);
        assert_eq!(session.slices.len(), 1);
        assert_eq!(session.slices[0].state, TherapySliceState::MaskOn);
        assert_eq!(
            session.slices[0].start_time_unix_ms,
            session.start_time.normalized_utc_unix_ms
        );
        assert_eq!(
            session.slices[0].end_time_unix_ms,
            session.end_time.normalized_utc_unix_ms
        );
        assert_eq!(
            session.start_time.device_local_wall_time,
            "2026-01-02T22:00:00.000"
        );
        assert_eq!(
            session.end_time.device_local_wall_time,
            "2026-01-02T23:00:00.000"
        );
        let mask_on_bytes = 600u16.to_be_bytes();
        let expected_source_key = opaque_key(
            "opap/resmed/str-mask-session-source/v1",
            [b"2026-01-02".as_slice(), mask_on_bytes.as_slice()],
        );
        assert_eq!(session.source_key, expected_source_key);
        assert_eq!(
            session.id,
            opaque_key(
                "opap/resmed/session-id/v2",
                std::iter::once(expected_source_key.as_bytes())
            )
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_str_boundary_only_session")
        );
        assert_eq!(
            report.statistics.files_read, 2,
            "identification and root STR are the two files read"
        );
        let expected_bytes =
            source.files["Identification.tgt"].len() + source.files["STR.edf"].len();
        assert_eq!(
            report.statistics.bytes_read,
            u64::try_from(expected_bytes).expect("fixture byte count")
        );

        let mut at_end = options();
        at_end.sessions_not_before_unix_ms = Some(session.end_time.normalized_utc_unix_ms);
        let skipped =
            import_resmed_sessions(&source, &at_end).expect("cutoff equal to exclusive STR end");
        assert!(skipped.sessions.is_empty());
        assert_eq!(skipped.statistics.sessions_skipped, 1);

        let mut before_end = options();
        before_end.sessions_not_before_unix_ms = Some(session.end_time.normalized_utc_unix_ms - 1);
        let retained =
            import_resmed_sessions(&source, &before_end).expect("cutoff before exclusive STR end");
        assert_eq!(retained.sessions.len(), 1);
    }

    #[test]
    fn repaired_str_session_keeps_source_provenance_and_scoped_warning() {
        let source = card_with_valid_str(&[600], &[0], []);
        let report = import_resmed_sessions(&source, &options()).expect("import repaired STR");
        assert_eq!(report.sessions.len(), 1);
        let session = &report.sessions[0];
        assert_eq!(session.data_kind, SessionDataKind::SummaryOnly);
        assert_eq!(session.summary.usage_ms, 14 * 60 * 60 * 1_000);
        assert_eq!(
            session.start_time.device_local_wall_time,
            "2026-01-02T22:00:00.000"
        );
        assert_eq!(
            session.end_time.device_local_wall_time,
            "2026-01-03T12:00:00.000"
        );
        let repair_warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "resmed_str_historical_trailing_noon_repair")
            .expect("session-scoped repair provenance");
        assert_eq!(
            repair_warning.session_id.as_deref(),
            Some(session.id.as_str())
        );
        assert!(repair_warning.message.contains("bounded"));
    }

    #[test]
    fn str_plus_brp_uses_exact_mask_usage_and_mask_off_mutation_keeps_identity() {
        let path = "DATALOG/20260102_220000_BRP.edf";
        let first_source = card_with_valid_str(&[600], &[660], [(path, simple_flow())]);
        let first = import_resmed_sessions(&first_source, &options()).expect("import STR plus BRP");
        assert_eq!(first.sessions.len(), 1);
        let first_session = &first.sessions[0];
        assert_eq!(first_session.data_kind, SessionDataKind::Partial);
        assert!(!first_session.waveforms.is_empty());
        assert_eq!(first_session.summary.usage_ms, 60 * 60 * 1_000);
        assert_eq!(first_session.slices.len(), 1);
        assert_eq!(
            first_session.slices[0].start_time_unix_ms,
            first_session.start_time.normalized_utc_unix_ms
        );
        assert_eq!(
            first_session.slices[0].end_time_unix_ms,
            first_session.end_time.normalized_utc_unix_ms
        );
        assert!(
            first
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_partial_str_session")
        );

        let changed_source = card_with_valid_str(&[600], &[720], [(path, simple_flow())]);
        let changed =
            import_resmed_sessions(&changed_source, &options()).expect("reimport changed mask-off");
        assert_eq!(changed.sessions.len(), 1);
        let changed_session = &changed.sessions[0];
        assert_eq!(changed_session.id, first_session.id);
        assert_eq!(changed_session.source_key, first_session.source_key);
        assert_eq!(
            changed_session.slices[0].source_key,
            first_session.slices[0].source_key
        );
        assert_eq!(changed_session.summary.usage_ms, 2 * 60 * 60 * 1_000);
        assert_eq!(
            changed_session.slices[0].start_time_unix_ms,
            first_session.slices[0].start_time_unix_ms
        );
        assert!(
            changed_session.slices[0].end_time_unix_ms > first_session.slices[0].end_time_unix_ms
        );
    }

    #[test]
    fn multiple_str_intervals_own_detail_files_without_cross_session_expansion() {
        let first = simple_flow();
        let second = synthetic_detail_with_start(
            &[SignalFixture::new("Flow", 2, &[-50, 50]).calibration(-1, 1, -50, 50)],
            1,
            "2",
            "ResMed SRN=serial-123",
            "03.01.2600.00.00",
        );
        let source = card_with_valid_str(
            &[600, 720],
            &[660, 780],
            [
                ("DATALOG/20260102_220000_BRP.edf", first),
                ("DATALOG/20260103_000000_BRP.edf", second),
            ],
        );
        let report = import_resmed_sessions(&source, &options()).expect("import two STR sessions");
        assert_eq!(report.sessions.len(), 2);
        assert!(
            report
                .sessions
                .iter()
                .all(|session| session.waveforms.len() == 1)
        );
        assert_eq!(
            report
                .sessions
                .iter()
                .map(|session| session.summary.usage_ms)
                .collect::<Vec<_>>(),
            [60 * 60 * 1_000, 60 * 60 * 1_000]
        );
        assert_ne!(report.sessions[0].id, report.sessions[1].id);
        assert_eq!(
            report.sessions[0].end_time.device_local_wall_time,
            "2026-01-02T23:00:00.000"
        );
        assert_eq!(
            report.sessions[1].start_time.device_local_wall_time,
            "2026-01-03T00:00:00.000"
        );
    }

    #[test]
    fn str_serial_mismatch_preserves_exact_brp_fallback_session_identity() {
        let fallback = import_resmed_sessions(&card_with_brp(simple_flow()), &options())
            .expect("invalid STR detail fallback");
        let mismatch_source = card_with_str_serial(
            "different-card",
            &[600],
            &[660],
            [("DATALOG/20260102_220000_BRP.edf", simple_flow())],
        );
        let mismatch =
            import_resmed_sessions(&mismatch_source, &options()).expect("mismatched STR fallback");
        assert_eq!(fallback.sessions.len(), 1);
        assert_eq!(mismatch.sessions.len(), 1);
        assert_eq!(mismatch.sessions[0].id, fallback.sessions[0].id);
        assert_eq!(
            mismatch.sessions[0].source_key,
            fallback.sessions[0].source_key
        );
        assert_eq!(
            mismatch.sessions[0].summary.usage_ms,
            fallback.sessions[0].summary.usage_ms
        );
        assert_eq!(mismatch.sessions[0].slices, fallback.sessions[0].slices);
        assert!(
            mismatch
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_str_serial_mismatch")
        );
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
    fn sad_and_sa2_share_calibrated_channels_but_keep_distinct_segment_provenance() {
        let import = |suffix: &str| {
            let path = match suffix {
                "SAD" => "DATALOG/20260102_220000_SAD.edf",
                "SA2" => "DATALOG/20260102_220000_SA2.edf",
                _ => unreachable!("fixture suffix"),
            };
            import_resmed_sessions(
                &card_with_detail_files([
                    ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
                    (path, simple_oximetry()),
                ]),
                &options(),
            )
            .expect("import BRP-backed oximetry")
        };
        let sad = import("SAD");
        let sa2 = import("SA2");
        let sad_session = &sad.sessions[0];
        let sa2_session = &sa2.sessions[0];
        assert_eq!(
            sad_session.source_key,
            "sha256:27bfc3cf634bdd1738c9dbd06f67be0e43a40f321c90e7a48f48ba980b647e1e"
        );
        assert_eq!(
            sad_session.id,
            "sha256:aded1b2399bd8ff4e0ec15cdcb27aeff59fe05b9da0f551d04a02598a955405f"
        );
        assert_eq!(
            sad_session
                .waveforms
                .iter()
                .find(|series| series.channel_id == FLOW_RATE_CHANNEL)
                .expect("BRP series")
                .source_key,
            "sha256:6253d7661ecd51dccb58141797ea27a0a4f6a763c48531266c96e1037f90daf1"
        );

        assert_eq!(sad_session.id, sa2_session.id);
        assert_eq!(sad_session.source_key, sa2_session.source_key);
        fn series<'a>(session: &'a Session, channel_id: &str) -> &'a WaveformSeries {
            session
                .waveforms
                .iter()
                .find(|series| series.channel_id == channel_id)
                .expect("oximetry series")
        }
        let sad_pulse = series(sad_session, PULSE_RATE_CHANNEL);
        let sa2_pulse = series(sa2_session, PULSE_RATE_CHANNEL);
        assert_eq!(sad_pulse.samples, vec![40.0, 90.0, 140.0, 65.0]);
        assert_eq!(sad_pulse.samples, sa2_pulse.samples);
        assert_eq!(sad_pulse.sample_interval_ms, 500.0);
        assert_eq!(sad_pulse.source_encoding, sa2_pulse.source_encoding);
        assert_eq!(
            sad_pulse.source_key,
            "sha256:f94af2f5286f02f52fc5bfa08c1ebd7fac6d9a88eb2d36d933ffc303995bb416"
        );
        assert_eq!(
            sa2_pulse.source_key,
            "sha256:d523e06e78089c410632a6bf570e2cfc2325ca885c2f98589571bdb35fe8538e"
        );

        let sad_spo2 = series(sad_session, OXYGEN_SATURATION_CHANNEL);
        let sa2_spo2 = series(sa2_session, OXYGEN_SATURATION_CHANNEL);
        assert_eq!(sad_spo2.samples, vec![80.0, 100.0]);
        assert_eq!(sad_spo2.samples, sa2_spo2.samples);
        assert_eq!(sad_spo2.sample_interval_ms, 1_000.0);
        assert_eq!(
            sad_spo2.source_encoding,
            Some(EdfSourceEncoding {
                digital_minimum: 0,
                digital_maximum: 10,
                physical_minimum: 80.0,
                physical_maximum: 100.0,
                samples_per_record: 1,
                record_duration_seconds: 1.0,
            })
        );
        assert_ne!(sad_spo2.source_key, sa2_spo2.source_key);
        for key in [
            &sad_pulse.source_key,
            &sa2_pulse.source_key,
            &sad_spo2.source_key,
            &sa2_spo2.source_key,
        ] {
            assert!(key.starts_with("sha256:"));
            assert_eq!(key.len(), 71);
            assert!(!key.contains("DATALOG"));
            assert!(!key.contains("serial-123"));
        }
    }

    #[test]
    fn oximetry_missing_sentinel_splits_leading_interior_and_trailing_gaps() {
        let oximetry = synthetic_brp(
            &[
                SignalFixture::new("Pulse", 8, &[-1, 20, 30, -1, 40, 50, -1, -1]),
                SignalFixture::new("SpO2", 8, &[-1; 8]),
                SignalFixture::new("Crc16", 8, &[0; 8]),
                SignalFixture::new("vendor mystery", 8, &[1; 8]),
            ],
            1,
            "8",
        );
        let report = import_resmed_sessions(
            &card_with_detail_files([
                ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
                ("DATALOG/20260102_220000_SAD.edf", oximetry),
            ]),
            &options(),
        )
        .expect("split missing oximetry samples");
        let session = &report.sessions[0];
        let segments: Vec<_> = session
            .waveforms
            .iter()
            .filter(|series| series.channel_id == PULSE_RATE_CHANNEL)
            .collect();
        assert_eq!(segments.len(), 2);
        let file_start = session
            .waveforms
            .iter()
            .find(|series| series.channel_id == FLOW_RATE_CHANNEL)
            .expect("BRP anchor")
            .start_time_unix_ms;
        assert_eq!(segments[0].start_time_unix_ms, file_start + 1_000);
        assert_eq!(segments[0].samples, vec![20.0, 30.0]);
        assert_eq!(segments[1].start_time_unix_ms, file_start + 4_000);
        assert_eq!(segments[1].samples, vec![40.0, 50.0]);
        assert!(
            session
                .waveforms
                .iter()
                .all(|series| series.channel_id != OXYGEN_SATURATION_CHANNEL)
        );
        assert!(
            session
                .channels
                .iter()
                .all(|channel| channel.id != OXYGEN_SATURATION_CHANNEL)
        );
        assert_eq!(session.summary.usage_ms, 2_000);
        assert_eq!(
            session.end_time.normalized_utc_unix_ms - session.start_time.normalized_utc_unix_ms,
            8_000
        );
        assert_eq!(
            report
                .warnings
                .iter()
                .filter(|warning| warning.code == "unknown_resmed_sad_signal")
                .count(),
            1
        );
        assert!(report.warnings.iter().all(|warning| {
            warning.code != "unknown_resmed_sad_signal" || !warning.message.contains("Crc16")
        }));
    }

    #[test]
    fn attached_oximetry_expands_the_envelope_without_inflating_brp_usage() {
        let oximetry = synthetic_detail_with_start(
            &[SignalFixture::new("Pulse", 4, &[60, 61, 62, 63])],
            1,
            "4",
            "ResMed SRN=serial-123",
            "02.01.2621.59.59",
        );
        let report = import_resmed_sessions(
            &card_with_detail_files([
                ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
                ("DATALOG/20260102_215959_SAD.edf", oximetry),
            ]),
            &options(),
        )
        .expect("attach overlapping oximetry");
        let session = &report.sessions[0];

        assert_eq!(
            session.start_time.device_local_wall_time,
            "2026-01-02T21:59:59.000"
        );
        assert_eq!(
            session.end_time.device_local_wall_time,
            "2026-01-02T22:00:03.000"
        );
        assert_eq!(session.summary.usage_ms, 2_000);
        assert_eq!(
            session.end_time.normalized_utc_unix_ms - session.start_time.normalized_utc_unix_ms,
            4_000
        );
    }

    #[test]
    fn malformed_sad_warns_without_discarding_valid_brp() {
        let mut malformed = simple_oximetry();
        malformed.pop();
        let report = import_resmed_sessions(
            &card_with_detail_files([
                ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
                ("DATALOG/20260102_220000_SAD.edf", malformed),
            ]),
            &options(),
        )
        .expect("malformed oximetry remains non-fatal");

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].waveforms.len(), 1);
        let warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "resmed_sad_not_decoded")
            .expect("stable SAD diagnostic");
        assert_eq!(
            warning.session_id.as_deref(),
            Some(report.sessions[0].id.as_str())
        );
    }

    #[test]
    fn mismatched_sad_serial_is_private_and_does_not_discard_valid_brp() {
        let oximetry = synthetic_brp_with_recording_id(
            &[SignalFixture::new("Pulse", 1, &[60, 61])],
            2,
            "1",
            "ResMed SRN=another-secret",
        );
        let report = import_resmed_sessions(
            &card_with_detail_files([
                ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
                ("DATALOG/20260102_220000_SAD.edf", oximetry),
            ]),
            &options(),
        )
        .expect("identity mismatch remains a per-file diagnostic");

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].waveforms.len(), 1);
        let mismatch = report
            .warnings
            .iter()
            .find(|warning| warning.code == "resmed_sad_serial_mismatch")
            .expect("stable mismatch warning");
        assert_eq!(
            mismatch.session_id.as_deref(),
            Some(report.sessions[0].id.as_str())
        );
        assert!(report.warnings.iter().all(|warning| {
            !warning.message.contains("serial-123") && !warning.message.contains("another-secret")
        }));
    }

    #[test]
    fn sad_without_valid_brp_never_creates_a_pap_session_or_reads_payload() {
        const SAD_PATH: &str = "DATALOG/20260102_220000_SAD.edf";
        let source = card_with_detail_files([(SAD_PATH, simple_oximetry())]);
        let report =
            import_resmed_sessions(&source, &options()).expect("SAD-only candidate is non-fatal");

        assert!(report.sessions.is_empty());
        assert_eq!(report.statistics.sessions_imported, 0);
        assert_eq!(source.full_read_count(SAD_PATH), 0);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_candidate_not_imported")
        );
    }

    #[test]
    fn oversized_sad_only_candidate_cannot_abort_a_later_valid_brp_session() {
        const SAD_PATH: &str = "DATALOG/20260101_220000_SAD.edf";
        const BRP_PATH: &str = "DATALOG/20260102_220000_BRP.edf";
        let mut old_sad = simple_oximetry();
        old_sad[168..184].copy_from_slice(b"01.01.2622.00.00");
        let mut source = card_with_detail_files([(SAD_PATH, old_sad), (BRP_PATH, simple_flow())]);
        source
            .inventory
            .entries
            .iter_mut()
            .find(|entry| entry.relative_path == SAD_PATH)
            .expect("SAD inventory entry")
            .size_bytes = RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT + 1;

        let report = import_resmed_sessions(&source, &options())
            .expect("unanchored optional oximetry is excluded from fatal budgets");
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].therapy_day, "2026-01-02");
        assert_eq!(source.full_read_count(SAD_PATH), 0);
        assert_eq!(source.full_read_count(BRP_PATH), 1);
    }

    #[test]
    fn oversized_early_attached_sad_does_not_starve_later_brp_decoding() {
        const EARLY_BRP_PATH: &str = "DATALOG/20260101_220000_BRP.edf";
        const EARLY_SAD_PATH: &str = "DATALOG/20260101_220000_SAD.edf";
        const LATER_BRP_PATH: &str = "DATALOG/20260102_220000_BRP.edf";
        let mut early_brp = simple_flow();
        early_brp[168..184].copy_from_slice(b"01.01.2622.00.00");
        let mut early_sad = simple_oximetry();
        early_sad[168..184].copy_from_slice(b"01.01.2622.00.00");
        let mut source = card_with_detail_files([
            (EARLY_BRP_PATH, early_brp),
            (EARLY_SAD_PATH, early_sad),
            (LATER_BRP_PATH, simple_flow()),
        ]);
        source
            .inventory
            .entries
            .iter_mut()
            .find(|entry| entry.relative_path == EARLY_SAD_PATH)
            .expect("SAD inventory entry")
            .size_bytes = RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT + 1;

        let report = import_resmed_sessions(&source, &options())
            .expect("all BRP anchors decode before best-effort oximetry");
        assert_eq!(report.sessions.len(), 2);
        assert_eq!(source.full_read_count(EARLY_BRP_PATH), 1);
        assert_eq!(source.full_read_count(LATER_BRP_PATH), 1);
        assert_eq!(source.full_read_count(EARLY_SAD_PATH), 0);
        let warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "resmed_sad_not_decoded")
            .expect("oversized optional SAD warning");
        assert!(warning.session_id.is_some());
    }

    #[test]
    fn compressed_oximetry_is_explicitly_unsupported_without_hiding_brp() {
        const COMPRESSED_PATH: &str = "DATALOG/20260102_220000_SAD.edf.gz";
        let source = card_with_detail_files([
            ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
            (COMPRESSED_PATH, simple_oximetry()),
        ]);
        let report = import_resmed_sessions(&source, &options())
            .expect("compressed oximetry remains a non-fatal indexing gap");

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].waveforms.len(), 1);
        assert_eq!(source.full_read_count(COMPRESSED_PATH), 0);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "compressed_edf_not_indexed")
        );
    }

    #[test]
    fn sad_complete_header_revalidation_applies_resmed_century_repair() {
        let mut oximetry = simple_oximetry();
        oximetry[168..184].copy_from_slice(b"02.01.8522.00.00");
        let report = import_resmed_sessions(
            &card_with_detail_files([
                ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
                ("DATALOG/20260102_220000_SAD.edf", oximetry),
            ]),
            &options(),
        )
        .expect("ResMed year 85 is revalidated consistently");

        assert!(
            report.sessions[0]
                .waveforms
                .iter()
                .any(|series| series.channel_id == PULSE_RATE_CHANNEL)
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "edf_header_time_in_future")
        );
    }

    #[test]
    fn logical_brp_identity_survives_corrected_content_while_child_identity_changes() {
        let first = card_with_brp(simple_flow());
        let corrected = synthetic_brp(
            &[SignalFixture::new("Flow", 2, &[-100, 0, 25, 100]).calibration(-4, 4, -100, 100)],
            2,
            "1",
        );
        let second = card_with_brp(corrected);
        let first_report = import_resmed_sessions(&first, &options()).expect("first import");
        let second_report = import_resmed_sessions(&second, &options()).expect("corrected import");
        let first_session = &first_report.sessions[0];
        let second_session = &second_report.sessions[0];

        assert_eq!(first_session.id, second_session.id);
        assert_eq!(first_session.source_key, second_session.source_key);
        assert_ne!(
            first_session.waveforms[0].source_key,
            second_session.waveforms[0].source_key
        );
        assert_ne!(
            first_session.waveforms[0].samples,
            second_session.waveforms[0].samples
        );
    }

    #[test]
    fn logical_brp_identity_distinguishes_same_anchor_time_with_different_basenames() {
        let flow_at = |path| {
            card_with_detail_files([(
                path,
                synthetic_detail_with_start(
                    &[SignalFixture::new("Flow", 1, &[0, 1])],
                    2,
                    "1",
                    "ResMed SRN=serial-123",
                    "02.01.2612.30.00",
                ),
            )])
        };
        let first = import_resmed_sessions(&flow_at("DATALOG/20260102_120000_BRP.edf"), &options())
            .expect("first basename");
        let second =
            import_resmed_sessions(&flow_at("DATALOG/20260102_130000_BRP.edf"), &options())
                .expect("second basename");

        assert_eq!(
            first.sessions[0].start_time.normalized_utc_unix_ms,
            second.sessions[0].start_time.normalized_utc_unix_ms
        );
        assert_eq!(
            first.sessions[0].therapy_day,
            second.sessions[0].therapy_day
        );
        assert_ne!(first.sessions[0].source_key, second.sessions[0].source_key);
        assert_ne!(first.sessions[0].id, second.sessions[0].id);
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
    fn cutoff_prefilter_uses_a_proven_upper_bound_for_all_decoded_details() {
        const OLD_PATH: &str = "DATALOG/20260101_220000_BRP.edf";
        const OLD_SAD_PATH: &str = "DATALOG/20260101_220000_SAD.edf";
        const ACTIVE_PATH: &str = "DATALOG/20260102_220000_BRP.edf";

        let active = simple_flow();
        let mut old = simple_flow();
        old[168..184].copy_from_slice(b"01.01.2622.00.00");
        let mut source = card_with_brp(active);
        source.insert(OLD_PATH, old);
        let mut old_sad = simple_oximetry();
        old_sad[168..184].copy_from_slice(b"01.01.2622.00.00");
        source.insert(OLD_SAD_PATH, old_sad);

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
            .expect("known upper bound permits a no-read cutoff");

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.statistics.sessions_skipped, 1);
        assert_eq!(source.full_read_count(OLD_PATH), 0);
        assert_eq!(source.full_read_count(OLD_SAD_PATH), 0);
        assert_eq!(source.full_read_count(ACTIVE_PATH), 1);
    }

    #[test]
    fn fractional_brp_end_survives_cutoff_at_the_index_truncated_second() {
        let source = card_with_brp(synthetic_brp(
            &[SignalFixture::new("Flow", 1, &[0])],
            1,
            "1.5",
        ));
        let start = DeviceLocalDateTime {
            year: 2026,
            month: 1,
            day: 2,
            hour: 22,
            minute: 0,
            second: 0,
            millisecond: 0,
        };
        let cutoff = normalize_millis(
            local_unix_millis(&start).expect("valid fixture start") + 1_000,
            &clock(),
        )
        .expect("normalize cutoff");
        let mut import_options = options();
        import_options.sessions_not_before_unix_ms = Some(cutoff);

        let report =
            import_resmed_sessions(&source, &import_options).expect("fractional BRP import");
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(
            report.sessions[0].end_time.normalized_utc_unix_ms,
            cutoff + 500
        );
        assert_eq!(report.statistics.sessions_skipped, 0);
    }

    #[test]
    fn fractional_and_unknown_count_sad_envelopes_survive_brp_end_cutoff() {
        let brp = synthetic_brp(&[SignalFixture::new("Flow", 1, &[0])], 1, "1");
        let known_fractional = synthetic_brp(&[SignalFixture::new("Pulse", 1, &[60])], 1, "1.5");
        let mut unknown_count = synthetic_brp(&[SignalFixture::new("Pulse", 1, &[60, 61])], 2, "1");
        unknown_count[236..244].copy_from_slice(&field("-1", 8));

        let start = DeviceLocalDateTime {
            year: 2026,
            month: 1,
            day: 2,
            hour: 22,
            minute: 0,
            second: 0,
            millisecond: 0,
        };
        let cutoff = normalize_millis(
            local_unix_millis(&start).expect("valid fixture start") + 1_000,
            &clock(),
        )
        .expect("normalize cutoff");
        for (sad, expected_extra_ms) in [(known_fractional, 500), (unknown_count, 1_000)] {
            let source = card_with_detail_files([
                ("DATALOG/20260102_220000_BRP.edf", brp.clone()),
                ("DATALOG/20260102_220000_SAD.edf", sad),
            ]);
            let mut import_options = options();
            import_options.sessions_not_before_unix_ms = Some(cutoff);
            let report = import_resmed_sessions(&source, &import_options)
                .expect("SAD envelope is decoded before cutoff");
            assert_eq!(report.sessions.len(), 1);
            assert_eq!(
                report.sessions[0].end_time.normalized_utc_unix_ms,
                cutoff + expected_extra_ms
            );
            assert_eq!(report.sessions[0].summary.usage_ms, 1_000);
        }
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
        let source = card_with_detail_files([
            ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
            (
                "DATALOG/20260102_220000_SA2.edf",
                synthetic_brp(
                    &[SignalFixture::new("Pulse", 6, &[-1, 60, 61, -1, 62, 63])],
                    1,
                    "6",
                ),
            ),
        ]);
        let first = import_resmed_sessions(&source, &options()).expect("first import");
        let second = import_resmed_sessions(&source, &options()).expect("second import");

        assert_eq!(first.sessions[0].id, second.sessions[0].id);
        assert_eq!(first.sessions[0].source_key, second.sessions[0].source_key);
        assert_eq!(
            first.sessions[0]
                .waveforms
                .iter()
                .map(|series| &series.source_key)
                .collect::<Vec<_>>(),
            second.sessions[0]
                .waveforms
                .iter()
                .map(|series| &series.source_key)
                .collect::<Vec<_>>()
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
    fn timezone_basis_is_bounded_by_utf8_byte_length_before_session_work() {
        let mut context = clock();
        context.timezone_basis = Some("x".repeat(MAX_TIMEZONE_BASIS_BYTES));
        validate_clock_context(&context).expect("exact ASCII byte boundary");
        context.timezone_basis = Some("é".repeat(MAX_TIMEZONE_BASIS_BYTES / 2));
        validate_clock_context(&context).expect("exact multibyte UTF-8 boundary");

        context
            .timezone_basis
            .as_mut()
            .expect("timezone basis")
            .push('x');
        let error = validate_clock_context(&context).expect_err("one byte beyond boundary");
        assert_eq!(error.kind, ImportErrorKind::InvalidConfiguration);
        assert!(error.message.contains("UTF-8 bytes"));

        let source = card_with_brp(simple_flow());
        let mut import_options = options();
        import_options
            .clock_context
            .as_mut()
            .expect("clock")
            .timezone_basis = context.timezone_basis;
        let error =
            import_resmed_sessions(&source, &import_options).expect_err("oversized basis is fatal");
        assert_eq!(error.kind, ImportErrorKind::InvalidConfiguration);
        assert_eq!(source.full_read_count("DATALOG/20260102_220000_BRP.edf"), 0);
    }

    #[test]
    fn aggregate_detail_budgets_cover_brp_sad_sa2_and_hostile_adapters() {
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

        let mut sad = file.clone();
        sad.kind = ResmedSessionFileKind::Sad;
        sad.relative_path = "DATALOG/budget_SAD.edf".to_owned();
        let mut sa2 = file.clone();
        sa2.kind = ResmedSessionFileKind::Sa2;
        sa2.relative_path = "DATALOG/budget_SA2.edf".to_owned();
        let all_detail_kinds = budget_index(vec![file.clone(), sad.clone(), sa2.clone()]);
        let mut detail_budget = DetailImportBudget::from_brp_index(&all_detail_kinds)
            .expect("only BRP is part of fatal preflight");
        assert_eq!(detail_budget.indexed_files, 1);
        assert_eq!(detail_budget.indexed_bytes, 1);
        detail_budget
            .reserve_optional_indexed_file(&sad)
            .expect("SAD fits remaining optional budget");
        detail_budget
            .reserve_optional_indexed_file(&sa2)
            .expect("SA2 fits remaining optional budget");
        assert_eq!(detail_budget.indexed_files, 3);
        assert_eq!(detail_budget.indexed_bytes, 3);

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
            DetailImportBudget::from_brp_index(&too_many_files)
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
            DetailImportBudget::from_brp_index(&too_many_bytes)
                .expect_err("aggregate byte budget")
                .kind,
            ImportErrorKind::SizeLimitExceeded
        );

        let aggregate_byte_limit = usize::try_from(RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT)
            .expect("aggregate byte budget fits usize");
        let mut actual = DetailImportBudget::default();
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
        let mut short_remainder = DetailImportBudget::default();
        short_remainder
            .charge_actual_bytes(aggregate_byte_limit - 7)
            .expect("leave a seven-byte remainder");
        assert_eq!(
            short_remainder
                .next_actual_read_limit()
                .expect("bounded next read"),
            7
        );

        let mut output = DetailImportBudget::default();
        output
            .reserve_output(
                RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT,
                RESMED_DETAIL_MAX_OUTPUT_SERIES_PER_IMPORT,
            )
            .expect("inclusive output boundaries");
        assert_eq!(
            output.reserve_output(1, 0).expect_err("sample budget").kind,
            ImportErrorKind::SizeLimitExceeded
        );
        assert_eq!(
            output.reserve_output(0, 1).expect_err("series budget").kind,
            ImportErrorKind::SizeLimitExceeded
        );
        assert_eq!(
            output.output_samples,
            RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT
        );
        assert_eq!(
            output.output_series,
            RESMED_DETAIL_MAX_OUTPUT_SERIES_PER_IMPORT
        );

        const SAD_PATH: &str = "DATALOG/20260102_220000_SAD.edf";
        let sad_bytes = simple_oximetry();
        let source = card_with_detail_files([
            ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
            (SAD_PATH, sad_bytes.clone()),
        ]);
        let clock_context = clock();
        let current = resmed_local_from_device(clock_context.current_device_local_time)
            .expect("valid fixture clock");
        let inventory = source.inventory().expect("fixture inventory");
        let index = index_session_candidates_from_inventory(&source, &inventory, &current)
            .expect("fixture candidate");
        let mut exhausted_actual = DetailImportBudget::default();
        exhausted_actual
            .charge_actual_bytes(aggregate_byte_limit)
            .expect("fill actual byte budget");
        let mut statistics = ImportStatistics::default();
        let fatal_actual = decode_brp_candidate(
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

        let mut exhausted_output = DetailImportBudget::default();
        exhausted_output
            .reserve_output(RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT, 0)
            .expect("fill aggregate output budget");
        let mut statistics = ImportStatistics::default();
        let fatal_output = decode_brp_candidate(
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

        let sad_file = index.candidates[0]
            .files
            .iter()
            .find(|file| file.kind == ResmedSessionFileKind::Sad)
            .expect("indexed SAD");
        let mut oximetry_output = DetailImportBudget::default();
        oximetry_output
            .reserve_output(RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT, 0)
            .expect("fill aggregate output budget");
        let mut statistics = ImportStatistics::default();
        let fatal_oximetry_output = decode_oximetry_file(
            &source,
            sad_file,
            "serial-123",
            &clock_context,
            &mut statistics,
            &mut oximetry_output,
        )
        .err()
        .expect("SAD shares aggregate output budget");
        assert_eq!(
            fatal_oximetry_output.kind,
            ImportErrorKind::SizeLimitExceeded
        );

        let hostile = LimitIgnoringSource {
            bytes: sad_bytes.clone(),
        };
        let mut hostile_budget = DetailImportBudget::default();
        hostile_budget
            .charge_actual_bytes(
                aggregate_byte_limit
                    .checked_sub(sad_bytes.len() - 1)
                    .expect("fixture fits aggregate budget"),
            )
            .expect("leave one byte less than the SAD payload");
        let mut statistics = ImportStatistics::default();
        let hostile_error = read_detail_edf(
            &hostile,
            sad_file,
            "serial-123",
            &mut statistics,
            &mut hostile_budget,
        )
        .err()
        .expect("adapter returning beyond the requested remainder fails closed");
        assert_eq!(hostile_error.kind, ImportErrorKind::SizeLimitExceeded);
        assert_eq!(statistics.files_read, 0);

        let oversized_file = budget_file(
            u64::try_from(RESMED_BRP_MAX_FILE_BYTES).expect("file budget fits u64") + 1,
        );
        let mut oversized_index = budget_index(vec![oversized_file]);
        let mut decode_budget = DetailImportBudget::from_brp_index(&oversized_index)
            .expect("aggregate budget still fits");
        let candidate = oversized_index.candidates.remove(0);
        let mut statistics = ImportStatistics::default();
        let fatal = decode_brp_candidate(
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
    fn optional_oximetry_sample_and_segment_exhaustion_retains_brp_session() {
        for exhausted_dimension in ["samples", "series"] {
            let source = card_with_detail_files([
                ("DATALOG/20260102_220000_BRP.edf", simple_flow()),
                ("DATALOG/20260102_220000_SAD.edf", simple_oximetry()),
            ]);
            let clock_context = clock();
            let current = resmed_local_from_device(clock_context.current_device_local_time)
                .expect("valid fixture clock");
            let inventory = source.inventory().expect("fixture inventory");
            let index = index_session_candidates_from_inventory(&source, &inventory, &current)
                .expect("fixture candidate");
            let candidate = &index.candidates[0];
            let mut budget =
                DetailImportBudget::from_brp_index(&index).expect("BRP fatal preflight");
            let mut statistics = ImportStatistics::default();
            let mut decoded = decode_brp_candidate(
                &source,
                candidate,
                "serial-123",
                &clock_context,
                &mut statistics,
                &mut budget,
            )
            .expect("decode BRP anchor");
            let mut session = decoded.session.take().expect("BRP-backed session");
            match exhausted_dimension {
                "samples" => {
                    budget.output_samples = RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT;
                }
                "series" => {
                    budget.output_series = RESMED_DETAIL_MAX_OUTPUT_SERIES_PER_IMPORT;
                }
                _ => unreachable!("fixture dimension"),
            }

            attach_candidate_oximetry(
                &source,
                candidate,
                "serial-123",
                &clock_context,
                &mut session,
                &mut decoded.warnings,
                &mut statistics,
                &mut budget,
            )
            .expect("optional limit is scoped to its SAD attachment");

            assert_eq!(session.waveforms.len(), 1, "{exhausted_dimension}");
            assert_eq!(session.waveforms[0].channel_id, FLOW_RATE_CHANNEL);
            assert_eq!(session.summary.usage_ms, 2_000);
            assert!(
                decoded
                    .warnings
                    .iter()
                    .any(|warning| warning.code == "resmed_sad_not_decoded"),
                "{exhausted_dimension}"
            );
        }
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
