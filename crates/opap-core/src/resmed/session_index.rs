// Copyright (C) 2011-2018 Mark Watkins
// Copyright (c) 2019-2025 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from OSCAR:
// https://gitlab.com/CrimsonNape/OSCAR-code
// Upstream commit: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
// Relevant upstream file:
// oscar/SleepLib/loader_plugins/resmed_loader.cpp (`ScanFiles`,
// `lookupEDFType`, `getEDFDuration`, and `ResDayTask::run`)
// Modified: 2026-07-23

//! Bounded, filesystem-independent indexing of ResMed DATALOG session files.
//!
//! This module intentionally stops at a **pre-import** candidate manifest.
//! Verified root `STR.edf` mask-on/mask-off records provide authoritative
//! therapy anchors; days without an unambiguous usable STR interval retain
//! OSCAR's EDF-duration overlap fallback. Candidates are still not imported
//! sessions, and this module does not claim complete OSCAR parity.
//!
//! OSCAR rejects EDF header times later than the host clock. To keep this core
//! deterministic and WebAssembly-compatible, callers supply the current
//! device-local wall clock explicitly; no native clock or UTC offset is
//! invented here. Compressed EDF input and clinical decoding of AEV or
//! unknown-suffix files are explicit gaps; those file types still participate
//! in duration grouping as they do in pinned OSCAR.
//!
//! OSCAR's `ResMedDay::files` is an unsorted `QHash`; OPAP intentionally sorts
//! source basenames to make candidate discovery reproducible across runs.
//! OPAP also intentionally validates the complete bounded EDF header and signal
//! descriptors with the shared parser. OSCAR's duration scan reads only three
//! fixed fields, so this safety hardening can reject malformed files that OSCAR
//! would still group from date, record count, and record duration alone.
//! Signal durations above seven days are likewise rejected as a bounded-input
//! policy; pinned OSCAR has no equivalent duration cap. For day-wide EVE/CSL
//! entries, the manifest preserves all recognized source files instead of
//! reproducing OSCAR's iteration-dependent timestamp-map overwrite.

use crate::{
    DeviceLocalDateTime, ImportError, ImportErrorKind, ImportSource, ImportWarning, SourceEntry,
    SourceEntryKind, SourceInventory, WarningSeverity,
};
use opap_edf::{EdfHeader, Parser};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::str::{
    RESMED_STR_MAX_FILE_BYTES, StrBoundaryDiagnostics, StrBoundaryRepair, StrDecodeError,
    decode_str,
};

const DATALOG_PREFIX: &str = "DATALOG/";
const STR_EDF_PATH: &str = "STR.edf";
const FIXED_EDF_HEADER_BYTES: usize = 256;
const EDF_SIGNAL_HEADER_BYTES: usize = 256;
const EDF_MAX_SIGNALS: usize = 256;
const HEADER_FILENAME_DRIFT_SECONDS: i64 = 6 * 60 * 60;
const EARLIEST_PLAUSIBLE_RESMED_YEAR: u16 = 2005;
const MAX_INDEXED_EDF_DURATION_MILLIS: u64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_DAY_FILE_ASSOCIATIONS: usize = RESMED_SESSION_INDEX_MAX_ENTRIES;
const MAX_FALLBACK_GROUP_COMPARISONS: usize = 4_000_000;

/// Version of the serialized ResMed candidate-manifest contract.
pub const RESMED_SESSION_INDEX_SCHEMA_VERSION: u16 = 3;

/// Largest prefix needed by the bounded EDF parser for 256 signal descriptors.
pub const RESMED_EDF_HEADER_MAX_BYTES: usize =
    FIXED_EDF_HEADER_BYTES + EDF_SIGNAL_HEADER_BYTES * EDF_MAX_SIGNALS;

/// Maximum number of portable inventory entries considered by one index pass.
pub const RESMED_SESSION_INDEX_MAX_ENTRIES: usize = 100_000;

/// Maximum UTF-8 byte length accepted for one portable inventory path.
pub const RESMED_SESSION_INDEX_MAX_PATH_BYTES: usize = 4_096;

/// A wall-clock time copied from device media without inventing a UTC offset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResmedDeviceLocalTime {
    /// ISO 8601 local wall time with no offset suffix.
    pub wall_time: String,
    /// Four-digit local calendar year.
    pub year: u16,
    /// Local calendar month, 1 through 12.
    pub month: u8,
    /// Local calendar day, 1 through 31.
    pub day: u8,
    /// Local hour, 0 through 23.
    pub hour: u8,
    /// Local minute, 0 through 59.
    pub minute: u8,
    /// Local second, 0 through 59.
    pub second: u8,
    /// Millisecond within the local second.
    pub millisecond: u16,
}

/// Recognized ResMed DATALOG EDF suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResmedSessionFileKind {
    /// High-resolution flow and pressure waveforms.
    Brp,
    /// Lower-resolution pressure, leak, and respiratory signals.
    Pld,
    /// Apnea and hypopnea annotations, commonly day-wide.
    Eve,
    /// Cheyne-Stokes respiration start/end annotations, commonly day-wide.
    Csl,
    /// Oximetry signals used by older cards.
    Sad,
    /// Oximetry signals using the newer suffix alias.
    Sa2,
    /// AEV annotations; they affect OSCAR grouping but payload loading is a no-op.
    Aev,
    /// A manufacturer suffix not known to the pinned loader's type table.
    Unknown,
}

/// Whether a file belongs to one session or is shared across a ResMed noon-day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResmedSessionFileScope {
    /// BRP, PLD, SAD, SA2, AEV, or unknown data grouped into one candidate.
    Session,
    /// EVE or CSL annotations attached to every candidate on the same noon-day.
    ResmedDay,
}

/// Clock source selected for grouping a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResmedTimestampSource {
    /// Valid EDF header time within OSCAR's six-hour filename tolerance.
    EdfHeader,
    /// Filename time used because the header was implausible/drifted or because
    /// OSCAR does not open annotation-only EVE/CSL files during duration scan.
    Filename,
}

/// Validated header fields needed for candidate discovery, without sample data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResmedEdfHeaderSummary {
    /// Device-local start copied from the EDF header when parseable.
    ///
    /// OSCAR can still use count/duration and fall back to the filename when
    /// only the EDF date/time field is corrupt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<ResmedDeviceLocalTime>,
    /// Validated complete EDF header size.
    pub header_bytes: u64,
    /// Number of signal descriptors in the header.
    pub signal_count: u16,
    /// Declared records, or `None` for EDF's `-1` sentinel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_record_count: Option<u64>,
    /// Duration of one record exactly as parsed from the EDF header.
    pub record_duration_seconds: f64,
    /// Estimated whole-file duration, truncated to OSCAR's whole-second
    /// grouping precision, when the record count is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_duration_millis: Option<u64>,
}

/// One recognized EDF source file participating in a candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResmedSessionFile {
    /// Forward-slash-separated path relative to the selected card root.
    pub relative_path: String,
    /// File size reported by the source inventory.
    pub size_bytes: u64,
    /// Semantic type derived from the ResMed suffix.
    pub kind: ResmedSessionFileKind,
    /// Whether this file is session-specific or shared across a noon-day.
    pub scope: ResmedSessionFileScope,
    /// Device-local timestamp encoded in the filename.
    pub filename_start_time: ResmedDeviceLocalTime,
    /// Validated bounded EDF header summary for duration-scanned signal files.
    ///
    /// EVE/CSL annotation-only files use `None`, matching OSCAR's choice not to
    /// open them in `getEDFDuration` before day-wide attachment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edf_header: Option<ResmedEdfHeaderSummary>,
    /// Timestamp selected for overlap grouping.
    pub selected_start_time: ResmedDeviceLocalTime,
    /// Whether the selected timestamp came from the header or filename.
    pub timestamp_source: ResmedTimestampSource,
}

/// Source of the interval that anchors one ResMed candidate.
///
/// The default preserves deserialization of schema-v2 manifests, whose
/// candidates were all derived from detail-file duration grouping.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResmedSessionAnchor {
    /// No trustworthy STR interval was available; detail grouping is heuristic.
    #[default]
    DetailFallback,
    /// A serial-verified, non-overlapping STR mask-on/mask-off interval.
    StrMask {
        /// Local-noon ResMed therapy-day bucket.
        therapy_day: String,
        /// Selected nonnegative mask-on minute after local noon.
        mask_on_minute: u16,
        /// Selected mask-off minute after local noon.
        ///
        /// This can be a derived following-noon boundary when `repair` is
        /// [`StrBoundaryRepair::HistoricalTrailingNoon`].
        mask_off_minute: u16,
        /// Original signed digital sample from the mask-on signal.
        ///
        /// Older schema-v3 manifests deserialize this as `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_mask_on_value: Option<i16>,
        /// Original signed digital sample from the mask-off signal.
        ///
        /// Older schema-v3 manifests deserialize this as `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_mask_off_value: Option<i16>,
        /// Selected authoritative device-local start.
        start_time: ResmedDeviceLocalTime,
        /// Selected authoritative device-local exclusive end.
        end_time: ResmedDeviceLocalTime,
        /// Bounded repair applied to the source samples, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repair: Option<StrBoundaryRepair>,
    },
}

/// A deterministic group of recognized EDF files that may form one session.
///
/// This is not an imported session. [`ResmedSessionAnchor`] distinguishes
/// authoritative STR intervals from the backward-compatible detail fallback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResmedSessionCandidate {
    /// Heuristic identifier deterministic for the same inventory and algorithm.
    ///
    /// It is not stable across index-algorithm changes, OSCAR's UTC session
    /// start, or a finalized imported-session ID.
    pub id: String,
    /// Earliest selected device-local file start in the heuristic group, not UTC.
    pub start_time: ResmedDeviceLocalTime,
    /// Latest estimated device-local file end when any duration is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_end_time: Option<ResmedDeviceLocalTime>,
    /// Authoritative device-local ResMed day in `YYYY-MM-DD`, split at noon.
    ///
    /// Consumers must carry this field; they must not rederive it from the
    /// candidate ID or from a later UTC conversion.
    pub resmed_day: String,
    /// Authoritative STR boundary or backward-compatible detail fallback.
    #[serde(default)]
    pub anchor: ResmedSessionAnchor,
    /// Session-specific files followed by shared day-wide annotations.
    pub files: Vec<ResmedSessionFile>,
}

/// Portable output of pre-import ResMed DATALOG candidate discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResmedSessionIndex {
    /// Serialized contract version.
    pub schema_version: u16,
    /// Candidates ordered by device-local start time and stable identifier.
    pub candidates: Vec<ResmedSessionCandidate>,
    /// Structured diagnostics for ignored, duplicate, or suspicious files.
    ///
    /// Their `session_id` remains unset because candidate IDs are not finalized
    /// imported-session IDs; `relative_path` provides the pre-import linkage.
    pub warnings: Vec<ImportWarning>,
}

#[derive(Debug, Clone)]
struct ParsedName {
    basename_key: String,
    kind: ResmedSessionFileKind,
    filename_time: ResmedDeviceLocalTime,
    filename_millis: i64,
    resmed_day: String,
}

enum ParseNameError {
    Invalid(&'static str),
}

#[derive(Debug, Clone)]
struct IndexedFile {
    file: ResmedSessionFile,
    filename_millis: i64,
    selected_millis: i64,
    /// End used by OSCAR-compatible interval matching. An unknown record count
    /// is represented as a zero-length point at the selected start.
    grouping_end_millis: Option<i64>,
    /// Actual duration-derived end exposed in the candidate DTO, when known.
    estimated_end_millis: Option<i64>,
    resmed_day: String,
}

#[derive(Debug)]
struct WorkingCandidate {
    interval_start_millis: i64,
    interval_end_millis: i64,
    resmed_day: String,
    anchor: ResmedSessionAnchor,
    files: Vec<IndexedFile>,
}

enum IndexFileOutcome {
    Indexed(Box<IndexedFile>),
    /// OSCAR performs readability and empty-file checks before basename
    /// deduplication, so another copy with the same basename may be tried.
    UnavailableBeforeDeduplication,
    /// The path was readable and therefore claimed its basename in ScanFiles,
    /// even though later duration validation could not index it.
    ClaimedButRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupMatch {
    FilenameWithinInterval,
    DurationOverlap,
}

/// Inventories a source and builds a bounded pre-import candidate manifest.
///
/// This compatibility entry point does not have a machine serial with which to
/// verify STR identity, so it emits only detail-fallback candidates. Call
/// [`index_session_candidates_for_machine`] when a trusted identification
/// record is available. `current_device_local_time` must be the host's current
/// local wall clock expressed without a UTC offset.
///
/// # Errors
///
/// Returns an error when the source cannot be inventoried or its portable
/// inventory exceeds index budgets. Individual EDF read or parse failures are
/// retained as warnings so one bad file cannot hide other candidates.
pub fn index_session_candidates(
    source: &dyn ImportSource,
    current_device_local_time: &ResmedDeviceLocalTime,
) -> Result<ResmedSessionIndex, ImportError> {
    let inventory = source.inventory()?;
    index_session_candidates_from_inventory(source, &inventory, current_device_local_time)
}

/// Inventories a source and indexes verified STR boundaries for one machine.
///
/// A missing, malformed, ambiguous, or serial-mismatched STR file is a soft
/// failure: the resulting manifest attempts bounded detail-fallback grouping.
/// If that compatibility pass exhausts its comparison budget, it fails closed
/// without suppressing any independently valid STR candidates.
///
/// # Errors
///
/// Returns an error for inventory-wide budget/configuration failures. Individual
/// STR and detail-file failures are retained as warnings.
pub fn index_session_candidates_for_machine(
    source: &dyn ImportSource,
    expected_serial: &str,
    current_device_local_time: &ResmedDeviceLocalTime,
) -> Result<ResmedSessionIndex, ImportError> {
    let inventory = source.inventory()?;
    index_session_candidates_from_inventory_for_machine(
        source,
        &inventory,
        expected_serial,
        current_device_local_time,
    )
}

/// Builds a candidate manifest from an existing inventory and bounded prefixes.
///
/// This function contains no native filesystem operations. Hosts may supply a
/// browser-backed or in-memory [`ImportSource`] for WebAssembly use.
///
/// # Errors
///
/// Returns [`ImportErrorKind::UnsupportedSource`] when the inventory has no
/// `DATALOG` directory or child, and [`ImportErrorKind::SizeLimitExceeded`]
/// when its entry or path budget is exceeded. An invalid supplied clock yields
/// [`ImportErrorKind::InvalidData`]. Per-file failures become manifest warnings.
pub fn index_session_candidates_from_inventory(
    source: &dyn ImportSource,
    inventory: &SourceInventory,
    current_device_local_time: &ResmedDeviceLocalTime,
) -> Result<ResmedSessionIndex, ImportError> {
    index_session_candidates_from_inventory_impl(source, inventory, None, current_device_local_time)
}

/// Builds a candidate manifest with serial-verified STR interval anchors.
///
/// The supplied serial must come from the selected card's identification
/// record. STR failures route otherwise-valid detail files through a bounded
/// compatibility grouping pass, which is omitted atomically on budget
/// exhaustion.
///
/// # Errors
///
/// Returns the same inventory/configuration errors as
/// [`index_session_candidates_from_inventory`], plus
/// [`ImportErrorKind::InvalidData`] for an empty expected serial.
pub fn index_session_candidates_from_inventory_for_machine(
    source: &dyn ImportSource,
    inventory: &SourceInventory,
    expected_serial: &str,
    current_device_local_time: &ResmedDeviceLocalTime,
) -> Result<ResmedSessionIndex, ImportError> {
    if expected_serial.trim().is_empty() {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "ResMed STR indexing requires a non-empty expected serial",
        ));
    }
    index_session_candidates_from_inventory_impl(
        source,
        inventory,
        Some(expected_serial.trim()),
        current_device_local_time,
    )
}

fn index_session_candidates_from_inventory_impl(
    source: &dyn ImportSource,
    inventory: &SourceInventory,
    expected_serial: Option<&str>,
    current_device_local_time: &ResmedDeviceLocalTime,
) -> Result<ResmedSessionIndex, ImportError> {
    if !valid_local_time(
        current_device_local_time.year,
        current_device_local_time.month,
        current_device_local_time.day,
        current_device_local_time.hour,
        current_device_local_time.minute,
        current_device_local_time.second,
    ) || current_device_local_time.millisecond > 999
    {
        return Err(ImportError::new(
            ImportErrorKind::InvalidData,
            "current device-local time is not a valid calendar time",
        ));
    }
    if inventory.entries.len() > RESMED_SESSION_INDEX_MAX_ENTRIES {
        return Err(ImportError::new(
            ImportErrorKind::SizeLimitExceeded,
            format!(
                "ResMed session index accepts at most {RESMED_SESSION_INDEX_MAX_ENTRIES} inventory entries; found {}",
                inventory.entries.len()
            ),
        ));
    }
    if let Some(entry) = inventory
        .entries
        .iter()
        .find(|entry| entry.relative_path.len() > RESMED_SESSION_INDEX_MAX_PATH_BYTES)
    {
        return Err(ImportError::new(
            ImportErrorKind::SizeLimitExceeded,
            format!(
                "ResMed session index paths may contain at most {RESMED_SESSION_INDEX_MAX_PATH_BYTES} UTF-8 bytes; found {}",
                entry.relative_path.len()
            ),
        ));
    }
    if !inventory.has_directory("DATALOG") {
        return Err(ImportError::new(
            ImportErrorKind::UnsupportedSource,
            "source has no ResMed DATALOG directory",
        ));
    }

    let mut warnings = Vec::new();
    let entry_groups = select_recognized_entries(inventory, &mut warnings);
    let mut session_files = Vec::new();
    let mut day_files = Vec::new();
    let current_device_local_millis = local_millis(current_device_local_time);

    for alternatives in entry_groups {
        for (index, (entry, parsed_name)) in alternatives.iter().enumerate() {
            let outcome = index_one_file(
                source,
                entry,
                parsed_name.clone(),
                current_device_local_millis,
                &mut warnings,
            );
            if matches!(&outcome, IndexFileOutcome::UnavailableBeforeDeduplication) {
                continue;
            }

            for (duplicate, _) in alternatives.iter().skip(index + 1) {
                warnings.push(warning(
                    "duplicate_resmed_edf",
                    format!(
                        "Duplicate ResMed filename ignored; using {}",
                        entry.relative_path
                    ),
                    Some(&duplicate.relative_path),
                ));
            }
            if let IndexFileOutcome::Indexed(indexed) = outcome {
                match indexed.file.scope {
                    ResmedSessionFileScope::Session => session_files.push(*indexed),
                    ResmedSessionFileScope::ResmedDay => day_files.push(*indexed),
                }
            }
            break;
        }
    }

    let mut str_candidates = expected_serial.map_or_else(Vec::new, |serial| {
        load_str_candidates(
            source,
            inventory,
            serial,
            current_device_local_time,
            &mut warnings,
        )
    });
    let unmatched_files =
        attach_session_files_to_str_candidates(&mut str_candidates, session_files);
    let mut candidates = str_candidates;
    candidates.extend(group_session_files(unmatched_files, &mut warnings));
    attach_day_files(&mut candidates, day_files, &mut warnings);
    candidates.sort_by(|left, right| {
        left.interval_start_millis
            .cmp(&right.interval_start_millis)
            .then_with(|| left.resmed_day.cmp(&right.resmed_day))
    });

    let mut candidates: Vec<_> = candidates.into_iter().map(finalize_candidate).collect();
    warnings.sort_by(|left, right| {
        left.relative_path
            .cmp(&right.relative_path)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });
    candidates.sort_by(|left, right| {
        local_millis(&left.start_time)
            .cmp(&local_millis(&right.start_time))
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(ResmedSessionIndex {
        schema_version: RESMED_SESSION_INDEX_SCHEMA_VERSION,
        candidates,
        warnings,
    })
}

fn select_recognized_entries<'a>(
    inventory: &'a SourceInventory,
    warnings: &mut Vec<ImportWarning>,
) -> Vec<Vec<(&'a SourceEntry, ParsedName)>> {
    let mut parsed = Vec::new();
    for entry in inventory
        .entries
        .iter()
        .filter(|entry| entry.kind == SourceEntryKind::File)
        .filter(|entry| entry.relative_path.starts_with(DATALOG_PREFIX))
    {
        let lowercase = entry.relative_path.to_ascii_lowercase();
        if (lowercase.ends_with(".edf") || lowercase.ends_with(".edf.gz"))
            && !is_supported_datalog_layout(&entry.relative_path)
        {
            warnings.push(warning(
                "unsupported_resmed_datalog_layout",
                "ResMed EDF ignored because OSCAR scans only DATALOG itself or one year/date directory below it",
                Some(&entry.relative_path),
            ));
            continue;
        }
        if lowercase.ends_with(".edf.gz") {
            warnings.push(warning(
                "compressed_edf_not_indexed",
                "Compressed ResMed EDF files are not indexed yet; use an uncompressed card copy",
                Some(&entry.relative_path),
            ));
            continue;
        }
        if !lowercase.ends_with(".edf") {
            continue;
        }
        if entry.size_bytes == 0 {
            warnings.push(warning(
                "empty_resmed_edf",
                "Empty ResMed EDF ignored before filename deduplication, matching OSCAR ScanFiles",
                Some(&entry.relative_path),
            ));
            continue;
        }
        match parse_resmed_name(&entry.relative_path) {
            Ok(name) => parsed.push((entry, name)),
            Err(ParseNameError::Invalid(message)) => warnings.push(warning(
                "invalid_resmed_edf_filename",
                message,
                Some(&entry.relative_path),
            )),
        }
    }

    parsed.sort_by(|(left_entry, left), (right_entry, right)| {
        left.basename_key
            .cmp(&right.basename_key)
            .then_with(|| {
                path_depth(&left_entry.relative_path).cmp(&path_depth(&right_entry.relative_path))
            })
            .then_with(|| left_entry.relative_path.cmp(&right_entry.relative_path))
    });

    let mut alternatives = BTreeMap::<String, Vec<(&SourceEntry, ParsedName)>>::new();
    for (entry, name) in parsed {
        alternatives
            .entry(name.basename_key.clone())
            .or_default()
            .push((entry, name));
    }
    alternatives.into_values().collect()
}

fn index_one_file(
    source: &dyn ImportSource,
    entry: &SourceEntry,
    parsed_name: ParsedName,
    current_device_local_millis: i64,
    warnings: &mut Vec<ImportWarning>,
) -> IndexFileOutcome {
    if entry.size_bytes == 0 {
        warnings.push(warning(
            "empty_resmed_edf",
            "Empty ResMed EDF ignored, matching OSCAR ScanFiles",
            Some(&entry.relative_path),
        ));
        return IndexFileOutcome::UnavailableBeforeDeduplication;
    }

    let scope = scope_for_kind(parsed_name.kind);
    if scope == ResmedSessionFileScope::ResmedDay {
        let file = ResmedSessionFile {
            relative_path: entry.relative_path.clone(),
            size_bytes: entry.size_bytes,
            kind: parsed_name.kind,
            scope,
            filename_start_time: parsed_name.filename_time.clone(),
            edf_header: None,
            selected_start_time: parsed_name.filename_time,
            timestamp_source: ResmedTimestampSource::Filename,
        };
        return IndexFileOutcome::Indexed(Box::new(IndexedFile {
            file,
            filename_millis: parsed_name.filename_millis,
            selected_millis: parsed_name.filename_millis,
            grouping_end_millis: None,
            estimated_end_millis: None,
            resmed_day: parsed_name.resmed_day,
        }));
    }

    let prefix = match source.read_file_prefix(&entry.relative_path, RESMED_EDF_HEADER_MAX_BYTES) {
        Ok(bytes) => bytes,
        Err(error) => {
            warnings.push(warning(
                "edf_header_read_failed",
                format!("Could not read bounded EDF header: {error}"),
                Some(&entry.relative_path),
            ));
            return IndexFileOutcome::UnavailableBeforeDeduplication;
        }
    };
    if prefix.len() > RESMED_EDF_HEADER_MAX_BYTES {
        warnings.push(warning(
            "edf_header_prefix_too_large",
            format!(
                "Source adapter returned {} header bytes, exceeding the {}-byte limit",
                prefix.len(),
                RESMED_EDF_HEADER_MAX_BYTES
            ),
            Some(&entry.relative_path),
        ));
        return IndexFileOutcome::ClaimedButRejected;
    }
    let (header, header_time) =
        match parse_header_with_datetime_fallback(&prefix, &parsed_name.filename_time) {
            Ok(value) => value,
            Err(error) => {
                warnings.push(warning(
                    "invalid_edf_header",
                    error.to_string(),
                    Some(&entry.relative_path),
                ));
                return IndexFileOutcome::ClaimedButRejected;
            }
        };

    match parsed_name.kind {
        ResmedSessionFileKind::Aev => warnings.push(warning(
            "resmed_aev_payload_not_decoded",
            "AEV participates in candidate grouping like pinned OSCAR, but its annotation payload is not decoded",
            Some(&entry.relative_path),
        )),
        ResmedSessionFileKind::Unknown => warnings.push(warning(
            "unknown_resmed_edf_payload_not_decoded",
            "Unknown EDF type participates in candidate grouping like pinned OSCAR, but its payload is not decoded",
            Some(&entry.relative_path),
        )),
        _ => {}
    }

    let (selected_start_time, selected_millis, timestamp_source) = match header_time.as_ref() {
        Some(header_time) => select_start_time(
            header_time,
            &parsed_name,
            current_device_local_millis,
            &entry.relative_path,
            warnings,
        ),
        None => {
            warnings.push(warning(
                "invalid_edf_header_time",
                "EDF start date/time is invalid; filename time selected while valid count/duration fields are retained",
                Some(&entry.relative_path),
            ));
            (
                parsed_name.filename_time.clone(),
                parsed_name.filename_millis,
                ResmedTimestampSource::Filename,
            )
        }
    };

    let estimated_duration_millis = estimated_duration_millis(&header);
    if header.declared_record_count.is_none() {
        warnings.push(warning(
            "edf_record_count_unknown",
            "EDF declares an unknown record count; OSCAR-compatible grouping treats its selected header/filename start as a zero-length point",
            Some(&entry.relative_path),
        ));
    } else if estimated_duration_millis.is_none() {
        warnings.push(warning(
            "edf_duration_out_of_range",
            "EDF duration exceeds the seven-day candidate-index bound or cannot be represented safely",
            Some(&entry.relative_path),
        ));
    }

    let estimated_end_millis = estimated_duration_millis
        .and_then(|duration| i64::try_from(duration).ok())
        .and_then(|duration| selected_millis.checked_add(duration));
    let grouping_end_millis = if header.declared_record_count.is_none() {
        Some(selected_millis)
    } else {
        estimated_end_millis
    };
    if scope == ResmedSessionFileScope::Session
        && header.declared_record_count.is_some()
        && estimated_duration_millis.is_none()
    {
        return IndexFileOutcome::ClaimedButRejected;
    }
    let file = ResmedSessionFile {
        relative_path: entry.relative_path.clone(),
        size_bytes: entry.size_bytes,
        kind: parsed_name.kind,
        scope,
        filename_start_time: parsed_name.filename_time,
        edf_header: Some(header_summary(
            &header,
            header_time,
            estimated_duration_millis,
        )),
        selected_start_time,
        timestamp_source,
    };

    IndexFileOutcome::Indexed(Box::new(IndexedFile {
        file,
        filename_millis: parsed_name.filename_millis,
        selected_millis,
        grouping_end_millis,
        estimated_end_millis,
        resmed_day: parsed_name.resmed_day,
    }))
}

fn parse_header_with_datetime_fallback(
    prefix: &[u8],
    filename_time: &ResmedDeviceLocalTime,
) -> Result<(EdfHeader, Option<ResmedDeviceLocalTime>), opap_edf::ParseError> {
    let parsed_header_time = prefix.get(168..184).and_then(parse_edf_header_time);
    let zero_record_duration = prefix
        .get(244..252)
        .and_then(parse_zero_record_duration)
        .unwrap_or(false);
    if parsed_header_time.is_some() && !zero_record_duration {
        return Parser::default()
            .parse_header(prefix)
            .map(|header| (header, parsed_header_time));
    }

    // Patch only source-tolerated fixed fields before invoking the strict
    // shared parser. This models OSCAR's filename fallback, avoids malformed
    // datetime arithmetic, and permits OSCAR's zero-duration grouping path
    // while retaining OPAP's complete-header safety validation.
    if prefix.len() < 252 {
        return Parser::default()
            .parse_header(prefix)
            .map(|header| (header, parsed_header_time));
    }
    let mut patched = prefix.to_vec();
    if parsed_header_time.is_none() {
        let replacement = format!(
            "{:02}.{:02}.{:02}{:02}.{:02}.{:02}",
            filename_time.day,
            filename_time.month,
            filename_time.year % 100,
            filename_time.hour,
            filename_time.minute,
            filename_time.second
        );
        patched[168..184].copy_from_slice(replacement.as_bytes());
    }
    if zero_record_duration {
        patched[244..252].copy_from_slice(b"1       ");
    }
    Parser::default().parse_header(&patched).map(|mut header| {
        if zero_record_duration {
            header.record_duration_seconds = 0.0;
        }
        (header, parsed_header_time)
    })
}

fn parse_zero_record_duration(bytes: &[u8]) -> Option<bool> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    let duration = text.parse::<f64>().ok()?;
    duration.is_finite().then_some(duration == 0.0)
}

fn parse_edf_header_time(bytes: &[u8]) -> Option<ResmedDeviceLocalTime> {
    if bytes.len() != 16
        || bytes[2] != b'.'
        || bytes[5] != b'.'
        || bytes[10] != b'.'
        || bytes[13] != b'.'
    {
        return None;
    }
    let pair = |start: usize| -> Option<u8> {
        let tens = bytes.get(start)?.checked_sub(b'0')?;
        let ones = bytes.get(start + 1)?.checked_sub(b'0')?;
        (tens <= 9 && ones <= 9).then(|| tens * 10 + ones)
    };
    let day = pair(0)?;
    let month = pair(3)?;
    let short_year = pair(6)?;
    // EDF's conventional pivot initially maps 85..99 to 1985..1999, after
    // which pinned getEDFDuration repairs every pre-2000 year by adding 100.
    let year = 2000 + u16::from(short_year);
    let hour = pair(8)?;
    let minute = pair(11)?;
    let second = pair(14)?;
    valid_local_time(year, month, day, hour, minute, second)
        .then(|| local_time(year, month, day, hour, minute, second, 0))
}

fn select_start_time(
    header_time: &ResmedDeviceLocalTime,
    parsed_name: &ParsedName,
    current_device_local_millis: i64,
    relative_path: &str,
    warnings: &mut Vec<ImportWarning>,
) -> (ResmedDeviceLocalTime, i64, ResmedTimestampSource) {
    let header_millis = local_millis(header_time);
    let header_too_early = header_time.year < EARLIEST_PLAUSIBLE_RESMED_YEAR;
    let header_in_future = header_millis > current_device_local_millis;
    let header_plausible = !header_too_early && !header_in_future;
    let drift_millis = header_millis.abs_diff(parsed_name.filename_millis);
    let drift_limit_millis =
        u64::try_from(HEADER_FILENAME_DRIFT_SECONDS * 1_000).expect("positive drift limit");
    if header_plausible && drift_millis <= drift_limit_millis {
        return (
            header_time.clone(),
            header_millis,
            ResmedTimestampSource::EdfHeader,
        );
    }

    let (code, message) = if header_plausible {
        (
            "edf_header_filename_drift",
            format!(
                "EDF header time {} differs from filename time {} by more than six hours; filename time selected",
                header_time.wall_time, parsed_name.filename_time.wall_time
            ),
        )
    } else if header_too_early {
        (
            "implausible_edf_header_time",
            format!(
                "EDF header time {} predates supported ResMed devices; filename time selected",
                header_time.wall_time
            ),
        )
    } else {
        (
            "edf_header_time_in_future",
            format!(
                "EDF header time {} is later than the supplied current device-local time {}; filename time selected",
                header_time.wall_time,
                local_time_from_millis(current_device_local_millis).wall_time
            ),
        )
    };
    warnings.push(warning(code, message, Some(relative_path)));
    (
        parsed_name.filename_time.clone(),
        parsed_name.filename_millis,
        ResmedTimestampSource::Filename,
    )
}

fn load_str_candidates(
    source: &dyn ImportSource,
    inventory: &SourceInventory,
    expected_serial: &str,
    current_device_local_time: &ResmedDeviceLocalTime,
    warnings: &mut Vec<ImportWarning>,
) -> Vec<WorkingCandidate> {
    let mut matching_entries = inventory
        .entries
        .iter()
        .filter(|entry| entry.kind == SourceEntryKind::File && entry.relative_path == STR_EDF_PATH);
    let Some(entry) = matching_entries.next() else {
        warnings.push(warning(
            "resmed_str_missing",
            "Root STR.edf is missing; bounded detail-file fallback grouping was attempted",
            Some(STR_EDF_PATH),
        ));
        return Vec::new();
    };
    if matching_entries.next().is_some() {
        warnings.push(warning(
            "resmed_str_duplicate_source",
            "Multiple root STR.edf inventory entries are ambiguous; bounded detail-file fallback grouping was attempted",
            Some(STR_EDF_PATH),
        ));
        return Vec::new();
    }
    if entry.size_bytes > u64::try_from(RESMED_STR_MAX_FILE_BYTES).expect("STR byte limit fits u64")
    {
        warnings.push(warning(
            "resmed_str_file_limit_exceeded",
            format!(
                "Root STR.edf exceeds the {RESMED_STR_MAX_FILE_BYTES}-byte input limit; bounded detail-file fallback grouping was attempted"
            ),
            Some(STR_EDF_PATH),
        ));
        return Vec::new();
    }

    let bytes = match source.read_file(STR_EDF_PATH, RESMED_STR_MAX_FILE_BYTES) {
        Ok(bytes) => bytes,
        Err(_) => {
            warnings.push(warning(
                "resmed_str_not_read",
                "Root STR.edf could not be read within its bounded limit; bounded detail-file fallback grouping was attempted",
                Some(STR_EDF_PATH),
            ));
            return Vec::new();
        }
    };
    if u64::try_from(bytes.len()).ok() != Some(entry.size_bytes) {
        warnings.push(warning(
            "resmed_str_changed_during_index",
            "Root STR.edf size changed after inventory; bounded detail-file fallback grouping was attempted",
            Some(STR_EDF_PATH),
        ));
        return Vec::new();
    }

    let current_device_local_time = DeviceLocalDateTime {
        year: current_device_local_time.year,
        month: current_device_local_time.month,
        day: current_device_local_time.day,
        hour: current_device_local_time.hour,
        minute: current_device_local_time.minute,
        second: current_device_local_time.second,
        millisecond: current_device_local_time.millisecond,
    };
    let decoded = match decode_str(&bytes, expected_serial, current_device_local_time) {
        Ok(decoded) => decoded,
        Err(error) => {
            push_str_decode_warning(error, warnings);
            return Vec::new();
        }
    };
    push_str_diagnostic_warnings(decoded.diagnostics, warnings);

    let boundary_count = decoded
        .days
        .iter()
        .try_fold(0usize, |count, day| count.checked_add(day.boundaries.len()))
        .unwrap_or(usize::MAX);
    if boundary_count > RESMED_SESSION_INDEX_MAX_ENTRIES {
        warnings.push(warning(
            "resmed_str_candidate_limit_exceeded",
            format!(
                "Root STR.edf contains more than {RESMED_SESSION_INDEX_MAX_ENTRIES} usable intervals; bounded detail-file fallback grouping was attempted"
            ),
            Some(STR_EDF_PATH),
        ));
        return Vec::new();
    }

    let mut candidates = Vec::new();
    if candidates.try_reserve_exact(boundary_count).is_err() {
        warnings.push(warning(
            "resmed_str_candidate_allocation_failed",
            "Root STR.edf intervals could not be allocated safely; bounded detail-file fallback grouping was attempted",
            Some(STR_EDF_PATH),
        ));
        return Vec::new();
    }
    for day in decoded.days {
        let noon = ResmedDeviceLocalTime {
            wall_time: format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}",
                day.local_noon.year,
                day.local_noon.month,
                day.local_noon.day,
                day.local_noon.hour,
                day.local_noon.minute,
                day.local_noon.second,
                day.local_noon.millisecond
            ),
            year: day.local_noon.year,
            month: day.local_noon.month,
            day: day.local_noon.day,
            hour: day.local_noon.hour,
            minute: day.local_noon.minute,
            second: day.local_noon.second,
            millisecond: day.local_noon.millisecond,
        };
        let noon_millis = local_millis(&noon);
        let therapy_day = format!(
            "{:04}-{:02}-{:02}",
            day.local_noon.year, day.local_noon.month, day.local_noon.day
        );
        for boundary in day.boundaries {
            let start_millis = noon_millis + i64::from(boundary.mask_on_minute) * 60_000;
            let end_millis = noon_millis + i64::from(boundary.mask_off_minute) * 60_000;
            let start_time = local_time_from_millis(start_millis);
            let end_time = local_time_from_millis(end_millis);
            candidates.push(WorkingCandidate {
                interval_start_millis: start_millis,
                interval_end_millis: end_millis,
                resmed_day: therapy_day.clone(),
                anchor: ResmedSessionAnchor::StrMask {
                    therapy_day: therapy_day.clone(),
                    mask_on_minute: boundary.mask_on_minute,
                    mask_off_minute: boundary.mask_off_minute,
                    source_mask_on_value: boundary.source_mask_on_value,
                    source_mask_off_value: boundary.source_mask_off_value,
                    start_time,
                    end_time,
                    repair: boundary.repair,
                },
                files: Vec::new(),
            });
        }
    }
    candidates.sort_by(|left, right| {
        left.interval_start_millis
            .cmp(&right.interval_start_millis)
            .then_with(|| left.interval_end_millis.cmp(&right.interval_end_millis))
    });
    candidates
}

fn push_str_decode_warning(error: StrDecodeError, warnings: &mut Vec<ImportWarning>) {
    let (code, message) = match error {
        StrDecodeError::SerialMismatch => (
            "resmed_str_serial_mismatch",
            "Root STR.edf serial does not match the selected card; bounded detail-file fallback grouping was attempted",
        ),
        StrDecodeError::MissingSerial | StrDecodeError::AmbiguousSerial => (
            "resmed_str_serial_ambiguous",
            "Root STR.edf has no unique serial identity; bounded detail-file fallback grouping was attempted",
        ),
        StrDecodeError::MissingSignal(_)
        | StrDecodeError::AmbiguousSignal(_)
        | StrDecodeError::NonDigitalSignal(_)
        | StrDecodeError::InvalidSamplesPerRecord { .. }
        | StrDecodeError::MaskSampleCountMismatch { .. } => (
            "resmed_str_signal_shape_rejected",
            "Root STR.edf does not contain one supported boundary-signal set; bounded detail-file fallback grouping was attempted",
        ),
        StrDecodeError::FileTooLarge { .. } => (
            "resmed_str_file_limit_exceeded",
            "Root STR.edf exceeds its bounded input limit; bounded detail-file fallback grouping was attempted",
        ),
        StrDecodeError::AllocationFailed { .. } => (
            "resmed_str_decode_allocation_failed",
            "Root STR.edf could not be decoded within allocation bounds; bounded detail-file fallback grouping was attempted",
        ),
        _ => (
            "resmed_str_not_decoded",
            "Root STR.edf is malformed or unsupported; bounded detail-file fallback grouping was attempted",
        ),
    };
    warnings.push(warning(code, message, Some(STR_EDF_PATH)));
}

fn push_str_diagnostic_warnings(
    diagnostics: StrBoundaryDiagnostics,
    warnings: &mut Vec<ImportWarning>,
) {
    let diagnostics = [
        (
            diagnostics.out_of_range_slots,
            "resmed_str_out_of_range_boundaries",
            "Out-of-range STR boundary slots were omitted",
        ),
        (
            diagnostics.invalid_pair_slots,
            "resmed_str_invalid_boundary_pairs",
            "Invalid STR boundary pairs were omitted",
        ),
        (
            diagnostics.unfinished_non_historical_slots,
            "resmed_str_unfinished_boundaries",
            "Unfinished current/future STR boundaries were omitted",
        ),
        (
            diagnostics.future_boundary_slots,
            "resmed_str_future_boundaries",
            "STR boundaries later than the supplied device-local clock were omitted",
        ),
        (
            diagnostics.repaired_historical_slots,
            "resmed_str_repaired_boundaries",
            "Historical unfinished STR boundaries were capped at the following noon",
        ),
        (
            diagnostics.ambiguous_days,
            "resmed_str_ambiguous_days",
            "Ambiguous STR therapy days were excluded from interval seeding",
        ),
        (
            diagnostics.mask_event_count_mismatch_days,
            "resmed_str_mask_count_mismatch",
            "STR mask-event counts disagreed with retained boundaries",
        ),
    ];
    for (count, code, message) in diagnostics {
        if count != 0 {
            warnings.push(warning(
                code,
                format!("{message} ({count})"),
                Some(STR_EDF_PATH),
            ));
        }
    }
}

fn attach_session_files_to_str_candidates(
    candidates: &mut [WorkingCandidate],
    files: Vec<IndexedFile>,
) -> Vec<IndexedFile> {
    let mut candidates_by_day = BTreeMap::<String, Vec<usize>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        candidates_by_day
            .entry(candidate.resmed_day.clone())
            .or_default()
            .push(index);
    }
    let mut unmatched = Vec::new();
    for file in files {
        if let Some(index) = find_str_candidate_match(&file, candidates, &candidates_by_day) {
            candidates[index].files.push(file);
        } else {
            unmatched.push(file);
        }
    }
    unmatched
}

fn find_str_candidate_match(
    file: &IndexedFile,
    candidates: &[WorkingCandidate],
    candidates_by_day: &BTreeMap<String, Vec<usize>>,
) -> Option<usize> {
    let day_indices = candidates_by_day.get(&file.resmed_day)?;
    let filename_position =
        upper_bound_candidate_start(day_indices, candidates, file.filename_millis);
    if let Some(&index) = filename_position
        .checked_sub(1)
        .and_then(|position| day_indices.get(position))
    {
        let candidate = &candidates[index];
        if file.filename_millis < candidate.interval_end_millis {
            return Some(index);
        }
    }

    let file_end = file.grouping_end_millis?;
    let overlap_position = lower_bound_candidate_start(day_indices, candidates, file_end);
    let &index = overlap_position
        .checked_sub(1)
        .and_then(|position| day_indices.get(position))?;
    (file.selected_millis < candidates[index].interval_end_millis).then_some(index)
}

fn upper_bound_candidate_start(
    indices: &[usize],
    candidates: &[WorkingCandidate],
    value: i64,
) -> usize {
    binary_candidate_start(indices, candidates, value, true)
}

fn lower_bound_candidate_start(
    indices: &[usize],
    candidates: &[WorkingCandidate],
    value: i64,
) -> usize {
    binary_candidate_start(indices, candidates, value, false)
}

fn binary_candidate_start(
    indices: &[usize],
    candidates: &[WorkingCandidate],
    value: i64,
    inclusive: bool,
) -> usize {
    binary_candidate_start_counted(indices, candidates, value, inclusive).0
}

fn binary_candidate_start_counted(
    indices: &[usize],
    candidates: &[WorkingCandidate],
    value: i64,
    inclusive: bool,
) -> (usize, usize) {
    let mut left = 0usize;
    let mut right = indices.len();
    let mut probes = 0usize;
    while left < right {
        probes += 1;
        let middle = left + (right - left) / 2;
        let candidate_start = candidates[indices[middle]].interval_start_millis;
        let belongs_left = if inclusive {
            candidate_start <= value
        } else {
            candidate_start < value
        };
        if belongs_left {
            left = middle + 1;
        } else {
            right = middle;
        }
    }
    (left, probes)
}

fn group_session_files(
    files: Vec<IndexedFile>,
    warnings: &mut Vec<ImportWarning>,
) -> Vec<WorkingCandidate> {
    group_session_files_with_limit(files, warnings, MAX_FALLBACK_GROUP_COMPARISONS)
}

fn group_session_files_with_limit(
    files: Vec<IndexedFile>,
    warnings: &mut Vec<ImportWarning>,
    comparison_limit: usize,
) -> Vec<WorkingCandidate> {
    let mut provisional_warnings = Vec::new();
    match try_group_session_files(files, &mut provisional_warnings, comparison_limit) {
        Ok(groups) => {
            warnings.extend(provisional_warnings);
            groups
        }
        Err(FallbackGroupingLimitExceeded) => {
            warnings.push(warning(
                "resmed_fallback_grouping_limit_exceeded",
                format!(
                    "Detail-fallback grouping exceeded the {comparison_limit}-comparison work limit; all detail-fallback candidates were omitted"
                ),
                None,
            ));
            Vec::new()
        }
    }
}

fn try_group_session_files(
    files: Vec<IndexedFile>,
    warnings: &mut Vec<ImportWarning>,
    comparison_limit: usize,
) -> Result<Vec<WorkingCandidate>, FallbackGroupingLimitExceeded> {
    let (mut bounded, mut unbounded): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|file| file.grouping_end_millis.is_some());
    bounded.sort_by(indexed_file_order);
    let mut groups: Vec<WorkingCandidate> = Vec::new();
    let mut groups_by_day = BTreeMap::<String, Vec<usize>>::new();
    let mut comparison_budget = FallbackGroupingBudget::new(comparison_limit);
    for file in bounded {
        let day_groups = groups_by_day
            .get(&file.resmed_day)
            .map_or(&[][..], Vec::as_slice);
        if let Some((index, match_kind)) =
            find_group_match(&file, &groups, day_groups, &mut comparison_budget)?
        {
            let group = &mut groups[index];
            if match_kind == GroupMatch::DurationOverlap {
                group.interval_start_millis = group.interval_start_millis.min(file.selected_millis);
                group.interval_end_millis = group
                    .interval_end_millis
                    .max(file.grouping_end_millis.expect("bounded file has an end"));
            }
            group.files.push(file);
        } else if file.grouping_end_millis != Some(file.selected_millis) {
            let group_index = groups.len();
            let resmed_day = file.resmed_day.clone();
            groups.push(WorkingCandidate {
                interval_start_millis: file.selected_millis,
                interval_end_millis: file.grouping_end_millis.expect("bounded file has an end"),
                resmed_day: resmed_day.clone(),
                anchor: ResmedSessionAnchor::DetailFallback,
                files: vec![file],
            });
            groups_by_day
                .entry(resmed_day)
                .or_default()
                .push(group_index);
        } else {
            warn_point_file_without_session(&file, warnings);
        }
    }

    unbounded.sort_by(indexed_file_order);
    for file in unbounded {
        let day_groups = groups_by_day
            .get(&file.resmed_day)
            .map_or(&[][..], Vec::as_slice);
        if let Some((index, _)) =
            find_group_match(&file, &groups, day_groups, &mut comparison_budget)?
        {
            groups[index].files.push(file);
        } else {
            warnings.push(warning(
                "unbounded_session_edf_not_indexed",
                "EDF with unknown duration was not promoted to a standalone session candidate",
                Some(&file.file.relative_path),
            ));
        }
    }
    Ok(groups)
}

fn find_group_match(
    file: &IndexedFile,
    groups: &[WorkingCandidate],
    day_groups: &[usize],
    comparison_budget: &mut FallbackGroupingBudget,
) -> Result<Option<(usize, GroupMatch)>, FallbackGroupingLimitExceeded> {
    // OSCAR's primary pass walks mask/fallback intervals forward and assigns a
    // file by its filename timestamp without expanding that interval.
    for &index in day_groups {
        comparison_budget.consume()?;
        let group = &groups[index];
        debug_assert_eq!(file.resmed_day, group.resmed_day);
        if file.filename_millis >= group.interval_start_millis
            && file.filename_millis < group.interval_end_millis
        {
            return Ok(Some((index, GroupMatch::FilenameWithinInterval)));
        }
    }

    // Only the duration-overlap fallback walks in reverse and expands the
    // chosen interval. There is deliberately no generic filename-lag cutoff.
    let Some(file_end) = file.grouping_end_millis else {
        return Ok(None);
    };
    for &index in day_groups.iter().rev() {
        comparison_budget.consume()?;
        let group = &groups[index];
        debug_assert_eq!(file.resmed_day, group.resmed_day);
        if group.interval_start_millis < file_end
            && file.selected_millis < group.interval_end_millis
        {
            return Ok(Some((index, GroupMatch::DurationOverlap)));
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FallbackGroupingLimitExceeded;

struct FallbackGroupingBudget {
    remaining_comparisons: usize,
}

impl FallbackGroupingBudget {
    const fn new(comparison_limit: usize) -> Self {
        Self {
            remaining_comparisons: comparison_limit,
        }
    }

    fn consume(&mut self) -> Result<(), FallbackGroupingLimitExceeded> {
        self.remaining_comparisons = self
            .remaining_comparisons
            .checked_sub(1)
            .ok_or(FallbackGroupingLimitExceeded)?;
        Ok(())
    }
}

fn warn_point_file_without_session(file: &IndexedFile, warnings: &mut Vec<ImportWarning>) {
    let unknown_record_count = file
        .file
        .edf_header
        .as_ref()
        .is_some_and(|header| header.declared_record_count.is_none());
    let (code, message) = if unknown_record_count {
        (
            "unbounded_session_edf_not_indexed",
            "EDF with unknown duration did not match an established session interval",
        )
    } else {
        (
            "zero_duration_session_edf",
            "Zero-duration BRP/PLD/SAD/SA2 file did not match an established session interval",
        )
    };
    warnings.push(warning(code, message, Some(&file.file.relative_path)));
}

fn attach_day_files(
    candidates: &mut [WorkingCandidate],
    day_files: Vec<IndexedFile>,
    warnings: &mut Vec<ImportWarning>,
) {
    attach_day_files_with_limit(candidates, day_files, warnings, MAX_DAY_FILE_ASSOCIATIONS);
}

fn attach_day_files_with_limit(
    candidates: &mut [WorkingCandidate],
    mut day_files: Vec<IndexedFile>,
    warnings: &mut Vec<ImportWarning>,
    association_limit: usize,
) {
    let mut candidates_by_day = BTreeMap::<String, Vec<usize>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        candidates_by_day
            .entry(candidate.resmed_day.clone())
            .or_default()
            .push(index);
    }
    day_files.sort_by(indexed_file_order);
    let mut files_by_day = BTreeMap::<String, Vec<IndexedFile>>::new();
    for file in day_files {
        if candidates_by_day.contains_key(&file.resmed_day) {
            files_by_day
                .entry(file.resmed_day.clone())
                .or_default()
                .push(file);
        } else {
            warnings.push(warning(
                "daywide_edf_without_session",
                "Day-wide EVE/CSL file has no STR or validated detail session candidate",
                Some(&file.file.relative_path),
            ));
        }
    }

    let required_associations = files_by_day.iter().try_fold(0usize, |total, (day, files)| {
        let candidate_count = candidates_by_day.get(day).map_or(0, Vec::len);
        candidate_count
            .checked_mul(files.len())
            .and_then(|count| total.checked_add(count))
    });
    if required_associations.is_none_or(|count| count > association_limit) {
        warnings.push(warning(
            "resmed_daywide_association_limit_exceeded",
            format!(
                "Day-wide EVE/CSL attachment exceeds the {association_limit}-association output limit; day-wide files were omitted"
            ),
            files_by_day
                .values()
                .flatten()
                .next()
                .map(|file| file.file.relative_path.as_str()),
        ));
        return;
    }

    for (day, files) in &files_by_day {
        for &candidate_index in &candidates_by_day[day] {
            if candidates[candidate_index]
                .files
                .try_reserve_exact(files.len())
                .is_err()
            {
                warnings.push(warning(
                    "resmed_daywide_association_allocation_failed",
                    "Day-wide EVE/CSL attachment could not be allocated safely; day-wide files were omitted",
                    files
                        .first()
                        .map(|file| file.file.relative_path.as_str()),
                ));
                return;
            }
        }
    }
    for (day, files) in files_by_day {
        for &candidate_index in &candidates_by_day[&day] {
            candidates[candidate_index]
                .files
                .extend(files.iter().cloned());
        }
    }
}

fn finalize_candidate(mut candidate: WorkingCandidate) -> ResmedSessionCandidate {
    candidate.files.sort_by(indexed_file_order);
    let (id, start_time, estimated_end_time) = match &candidate.anchor {
        ResmedSessionAnchor::DetailFallback => {
            let start_millis = candidate
                .files
                .iter()
                .filter(|file| file.file.scope == ResmedSessionFileScope::Session)
                .map(|file| file.selected_millis)
                .min()
                .expect("fallback candidate has at least one session-specific file");
            let estimated_end_millis = candidate
                .files
                .iter()
                .filter(|file| file.file.scope == ResmedSessionFileScope::Session)
                .filter_map(|file| file.estimated_end_millis)
                .max();
            let start_time = local_time_from_millis(start_millis);
            let source_filename = candidate
                .files
                .iter()
                .filter(|file| file.file.scope == ResmedSessionFileScope::Session)
                .filter_map(|file| file.file.relative_path.rsplit('/').next())
                .map(str::to_owned)
                .min()
                .expect("fallback candidate has at least one session-specific file");
            let id = format!(
                "resmed-local-{:04}{:02}{:02}-{:02}{:02}{:02}-file-{}",
                start_time.year,
                start_time.month,
                start_time.day,
                start_time.hour,
                start_time.minute,
                start_time.second,
                hex_identifier_component(&source_filename)
            );
            (
                id,
                start_time,
                estimated_end_millis.map(local_time_from_millis),
            )
        }
        ResmedSessionAnchor::StrMask {
            therapy_day,
            mask_on_minute,
            start_time,
            end_time,
            ..
        } => {
            let authoritative_start_millis = local_millis(start_time);
            let authoritative_end_millis = local_millis(end_time);
            let envelope_start_millis = candidate
                .files
                .iter()
                .filter(|file| file.file.scope == ResmedSessionFileScope::Session)
                .map(|file| file.selected_millis)
                .fold(authoritative_start_millis, i64::min);
            let envelope_end_millis = candidate
                .files
                .iter()
                .filter(|file| file.file.scope == ResmedSessionFileScope::Session)
                .filter_map(|file| file.estimated_end_millis)
                .fold(authoritative_end_millis, i64::max);
            (
                format!(
                    "resmed-str-mask-{}-{mask_on_minute:04}",
                    therapy_day.replace('-', "")
                ),
                local_time_from_millis(envelope_start_millis),
                Some(local_time_from_millis(envelope_end_millis)),
            )
        }
    };
    ResmedSessionCandidate {
        id,
        start_time,
        estimated_end_time,
        resmed_day: candidate.resmed_day,
        anchor: candidate.anchor,
        files: candidate.files.into_iter().map(|file| file.file).collect(),
    }
}

fn hex_identifier_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len().saturating_mul(2));
    for byte in value.bytes() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn parse_resmed_name(relative_path: &str) -> Result<ParsedName, ParseNameError> {
    let basename = relative_path
        .rsplit('/')
        .next()
        .ok_or(ParseNameError::Invalid("ResMed EDF path has no filename"))?;
    let stem = basename
        .get(..basename.len().saturating_sub(4))
        .ok_or(ParseNameError::Invalid("ResMed EDF filename is too short"))?;
    let fields: Vec<_> = stem.split('_').collect();
    if fields.len() < 3 {
        return Err(ParseNameError::Invalid(
            "ResMed EDF filename must contain YYYYMMDD_HHMMSS_TYPE",
        ));
    }
    let filename_time = parse_filename_time(fields[0], fields[1]).ok_or(
        ParseNameError::Invalid("ResMed EDF filename contains an invalid local date or time"),
    )?;
    let kind = parse_kind(fields.last().copied().unwrap_or_default());
    let filename_millis = local_millis(&filename_time);
    Ok(ParsedName {
        basename_key: basename.to_owned(),
        kind,
        resmed_day: resmed_day(&filename_time),
        filename_time,
        filename_millis,
    })
}

fn parse_kind(value: &str) -> ResmedSessionFileKind {
    if value.eq_ignore_ascii_case("BRP") {
        ResmedSessionFileKind::Brp
    } else if value.eq_ignore_ascii_case("PLD") {
        ResmedSessionFileKind::Pld
    } else if value.eq_ignore_ascii_case("EVE") {
        ResmedSessionFileKind::Eve
    } else if value.eq_ignore_ascii_case("CSL") {
        ResmedSessionFileKind::Csl
    } else if value.eq_ignore_ascii_case("SAD") {
        ResmedSessionFileKind::Sad
    } else if value.eq_ignore_ascii_case("SA2") {
        ResmedSessionFileKind::Sa2
    } else if value.eq_ignore_ascii_case("AEV") {
        ResmedSessionFileKind::Aev
    } else {
        ResmedSessionFileKind::Unknown
    }
}

const fn scope_for_kind(kind: ResmedSessionFileKind) -> ResmedSessionFileScope {
    match kind {
        ResmedSessionFileKind::Eve | ResmedSessionFileKind::Csl => {
            ResmedSessionFileScope::ResmedDay
        }
        ResmedSessionFileKind::Brp
        | ResmedSessionFileKind::Pld
        | ResmedSessionFileKind::Sad
        | ResmedSessionFileKind::Sa2
        | ResmedSessionFileKind::Aev
        | ResmedSessionFileKind::Unknown => ResmedSessionFileScope::Session,
    }
}

fn header_summary(
    header: &EdfHeader,
    start_time: Option<ResmedDeviceLocalTime>,
    estimated_duration_millis: Option<u64>,
) -> ResmedEdfHeaderSummary {
    ResmedEdfHeaderSummary {
        start_time,
        header_bytes: u64::try_from(header.header_bytes).expect("EDF header limit fits u64"),
        signal_count: u16::try_from(header.signals.len()).expect("EDF signal limit fits u16"),
        declared_record_count: header
            .declared_record_count
            .and_then(|count| u64::try_from(count).ok()),
        record_duration_seconds: header.record_duration_seconds,
        estimated_duration_millis,
    }
}

fn estimated_duration_millis(header: &EdfHeader) -> Option<u64> {
    let records = header.declared_record_count?;
    let records_u32 = u32::try_from(records).ok()?;
    let seconds = header.record_duration_seconds * f64::from(records_u32);
    if !seconds.is_finite()
        || seconds < 0.0
        || seconds > MAX_INDEXED_EDF_DURATION_MILLIS as f64 / 1_000.0
    {
        return None;
    }
    // Pinned OSCAR assigns the floating-point end expression to quint32, which
    // truncates positive fractional seconds. Keep grouping on that same whole-
    // second boundary rather than rounding to the nearest millisecond.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole_seconds = seconds as u64;
    whole_seconds.checked_mul(1_000)
}

fn indexed_file_order(left: &IndexedFile, right: &IndexedFile) -> std::cmp::Ordering {
    let left_basename = left
        .file
        .relative_path
        .rsplit('/')
        .next()
        .unwrap_or(&left.file.relative_path);
    let right_basename = right
        .file
        .relative_path
        .rsplit('/')
        .next()
        .unwrap_or(&right.file.relative_path);
    left.resmed_day
        .cmp(&right.resmed_day)
        .then_with(|| scope_order(left.file.scope).cmp(&scope_order(right.file.scope)))
        .then_with(|| left.filename_millis.cmp(&right.filename_millis))
        .then_with(|| left_basename.cmp(right_basename))
        .then_with(|| left.file.relative_path.cmp(&right.file.relative_path))
}

const fn scope_order(scope: ResmedSessionFileScope) -> u8 {
    match scope {
        ResmedSessionFileScope::Session => 0,
        ResmedSessionFileScope::ResmedDay => 1,
    }
}

fn warning(
    code: impl Into<String>,
    message: impl Into<String>,
    relative_path: Option<&str>,
) -> ImportWarning {
    ImportWarning {
        code: code.into(),
        severity: WarningSeverity::Warning,
        message: message.into(),
        relative_path: relative_path.map(str::to_owned),
        session_id: None,
    }
}

fn path_depth(path: &str) -> usize {
    path.bytes().filter(|byte| *byte == b'/').count()
}

fn is_supported_datalog_layout(path: &str) -> bool {
    let Some(relative) = path.strip_prefix(DATALOG_PREFIX) else {
        return false;
    };
    let components: Vec<_> = relative.split('/').collect();
    match components.as_slice() {
        [filename] => !filename.is_empty(),
        [directory, filename] if !filename.is_empty() => {
            (directory.len() == 4 && directory.bytes().all(|byte| byte.is_ascii_digit()))
                || (directory.len() == 8 && parse_filename_time(directory, "000000").is_some())
        }
        _ => false,
    }
}

fn parse_filename_time(date: &str, time: &str) -> Option<ResmedDeviceLocalTime> {
    if date.len() != 8 || time.len() != 6 {
        return None;
    }
    let year = parse_decimal::<u16>(date.get(0..4)?)?;
    let month = parse_decimal::<u8>(date.get(4..6)?)?;
    let day = parse_decimal::<u8>(date.get(6..8)?)?;
    let hour = parse_decimal::<u8>(time.get(0..2)?)?;
    let minute = parse_decimal::<u8>(time.get(2..4)?)?;
    let second = parse_decimal::<u8>(time.get(4..6)?)?;
    valid_local_time(year, month, day, hour, minute, second)
        .then(|| local_time(year, month, day, hour, minute, second, 0))
}

fn parse_decimal<T: std::str::FromStr>(value: &str) -> Option<T> {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| value.parse().ok())
        .flatten()
}

fn local_time(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    millisecond: u16,
) -> ResmedDeviceLocalTime {
    let wall_time = if millisecond == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}")
    };
    ResmedDeviceLocalTime {
        wall_time,
        year,
        month,
        day,
        hour,
        minute,
        second,
        millisecond,
    }
}

fn valid_local_time(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> bool {
    year > 0
        && (1..=12).contains(&month)
        && day > 0
        && day <= days_in_month(year, month)
        && hour < 24
        && minute < 60
        && second < 60
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

fn local_millis(value: &ResmedDeviceLocalTime) -> i64 {
    days_from_civil(value.year, value.month, value.day) * 86_400_000
        + i64::from(value.hour) * 3_600_000
        + i64::from(value.minute) * 60_000
        + i64::from(value.second) * 1_000
        + i64::from(value.millisecond)
}

fn local_time_from_millis(value: i64) -> ResmedDeviceLocalTime {
    let days = value.div_euclid(86_400_000);
    let within_day = value.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = u8::try_from(within_day / 3_600_000).expect("hour in range");
    let minute = u8::try_from((within_day % 3_600_000) / 60_000).expect("minute in range");
    let second = u8::try_from((within_day % 60_000) / 1_000).expect("second in range");
    let millisecond = u16::try_from(within_day % 1_000).expect("millisecond in range");
    local_time(year, month, day, hour, minute, second, millisecond)
}

fn resmed_day(time: &ResmedDeviceLocalTime) -> String {
    let mut day_millis = days_from_civil(time.year, time.month, time.day) * 86_400_000;
    if time.hour < 12 {
        day_millis -= 86_400_000;
    }
    let day = local_time_from_millis(day_millis);
    format!("{:04}-{:02}-{:02}", day.year, day.month, day.day)
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

fn civil_from_days(days: i64) -> (u16, u8, u8) {
    let zero_day = days + 719_468;
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
    (
        u16::try_from(year).expect("ResMed year in u16 range"),
        u8::try_from(month).expect("month in range"),
        u8::try_from(day).expect("day in range"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Clone, Default)]
    struct MemorySource {
        entries: Vec<SourceEntry>,
        files: BTreeMap<String, Vec<u8>>,
    }

    fn fixture_current_time() -> ResmedDeviceLocalTime {
        local_time(2030, 12, 31, 23, 59, 59, 999)
    }

    fn index_session_candidates(
        source: &dyn ImportSource,
    ) -> Result<ResmedSessionIndex, ImportError> {
        super::index_session_candidates(source, &fixture_current_time())
    }

    fn index_session_candidates_from_inventory(
        source: &dyn ImportSource,
        inventory: &SourceInventory,
    ) -> Result<ResmedSessionIndex, ImportError> {
        super::index_session_candidates_from_inventory(source, inventory, &fixture_current_time())
    }

    fn index_session_candidates_for_machine(
        source: &dyn ImportSource,
        expected_serial: &str,
    ) -> Result<ResmedSessionIndex, ImportError> {
        super::index_session_candidates_for_machine(
            source,
            expected_serial,
            &fixture_current_time(),
        )
    }

    impl MemorySource {
        fn insert(&mut self, path: &str, bytes: Vec<u8>) {
            self.entries.push(SourceEntry {
                relative_path: path.to_owned(),
                kind: SourceEntryKind::File,
                size_bytes: u64::try_from(bytes.len()).expect("fixture length"),
            });
            self.files.insert(path.to_owned(), bytes);
        }

        fn insert_directory(&mut self, path: &str) {
            self.entries.push(SourceEntry {
                relative_path: path.to_owned(),
                kind: SourceEntryKind::Directory,
                size_bytes: 0,
            });
        }
    }

    impl ImportSource for MemorySource {
        fn inventory(&self) -> Result<SourceInventory, ImportError> {
            Ok(SourceInventory {
                entries: self.entries.clone(),
                total_file_bytes: self.entries.iter().map(|entry| entry.size_bytes).sum(),
            })
        }

        fn read_file(&self, relative_path: &str, max_bytes: usize) -> Result<Vec<u8>, ImportError> {
            let bytes = self.files.get(relative_path).ok_or_else(|| {
                ImportError::new(ImportErrorKind::Source, "missing fixture").at_path(relative_path)
            })?;
            if bytes.len() > max_bytes {
                return Err(ImportError::new(
                    ImportErrorKind::SizeLimitExceeded,
                    "fixture exceeds complete-file limit",
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
                ImportError::new(ImportErrorKind::Source, "missing fixture").at_path(relative_path)
            })?;
            Ok(bytes[..bytes.len().min(max_bytes)].to_vec())
        }
    }

    #[test]
    fn schema_v3_anchor_is_typed_and_schema_v2_candidates_default_to_detail_fallback() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );
        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.schema_version, 3);
        assert_eq!(
            index.candidates[0].anchor,
            ResmedSessionAnchor::DetailFallback
        );

        let mut legacy = serde_json::to_value(&index).expect("serialize manifest");
        legacy["schema_version"] = serde_json::json!(2);
        legacy["candidates"][0]
            .as_object_mut()
            .expect("candidate object")
            .remove("anchor");
        let decoded: ResmedSessionIndex =
            serde_json::from_value(legacy).expect("schema-v2 manifest remains readable");
        assert_eq!(decoded.schema_version, 2);
        assert_eq!(
            decoded.candidates[0].anchor,
            ResmedSessionAnchor::DetailFallback
        );

        let anchor = ResmedSessionAnchor::StrMask {
            therapy_day: "2026-01-01".to_owned(),
            mask_on_minute: 600,
            mask_off_minute: 660,
            source_mask_on_value: Some(600),
            source_mask_off_value: Some(660),
            start_time: local_time(2026, 1, 1, 22, 0, 0, 0),
            end_time: local_time(2026, 1, 1, 23, 0, 0, 0),
            repair: None,
        };
        let mut legacy_anchor = serde_json::to_value(anchor).expect("serialize v3 anchor");
        let object = legacy_anchor.as_object_mut().expect("anchor object");
        object.remove("source_mask_on_value");
        object.remove("source_mask_off_value");
        object.remove("repair");
        let decoded: ResmedSessionAnchor =
            serde_json::from_value(legacy_anchor).expect("earlier v3 anchor remains readable");
        let ResmedSessionAnchor::StrMask {
            source_mask_on_value,
            source_mask_off_value,
            repair,
            ..
        } = decoded
        else {
            panic!("typed STR anchor");
        };
        assert_eq!(source_mask_on_value, None);
        assert_eq!(source_mask_off_value, None);
        assert_eq!(repair, None);
    }

    #[test]
    fn verified_str_creates_summary_candidate_with_exact_raw_anchor() {
        let mut source = MemorySource::default();
        source.insert_directory("DATALOG");
        source.insert("STR.edf", synthetic_str("serial-123", &[600], &[660]));

        let index = index_session_candidates_for_machine(&source, "serial-123").expect("STR index");
        assert!(index.warnings.is_empty());
        assert_eq!(index.candidates.len(), 1);
        let candidate = &index.candidates[0];
        assert!(candidate.files.is_empty());
        assert_eq!(candidate.id, "resmed-str-mask-20260101-0600");
        assert_eq!(candidate.resmed_day, "2026-01-01");
        assert_eq!(candidate.start_time.wall_time, "2026-01-01T22:00:00");
        assert_eq!(
            candidate
                .estimated_end_time
                .as_ref()
                .expect("STR end")
                .wall_time,
            "2026-01-01T23:00:00"
        );
        assert_eq!(
            candidate.anchor,
            ResmedSessionAnchor::StrMask {
                therapy_day: "2026-01-01".to_owned(),
                mask_on_minute: 600,
                mask_off_minute: 660,
                source_mask_on_value: Some(600),
                source_mask_off_value: Some(660),
                start_time: local_time(2026, 1, 1, 22, 0, 0, 0),
                end_time: local_time(2026, 1, 1, 23, 0, 0, 0),
                repair: None,
            }
        );
    }

    #[test]
    fn repaired_str_anchor_preserves_source_values_and_selected_boundary_provenance() {
        let mut source = MemorySource::default();
        source.insert_directory("DATALOG");
        source.insert("STR.edf", synthetic_str("serial-123", &[600], &[0]));

        let index = index_session_candidates_for_machine(&source, "serial-123").expect("STR index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(
            index.candidates[0].anchor,
            ResmedSessionAnchor::StrMask {
                therapy_day: "2026-01-01".to_owned(),
                mask_on_minute: 600,
                mask_off_minute: 1_440,
                source_mask_on_value: Some(600),
                source_mask_off_value: Some(0),
                start_time: local_time(2026, 1, 1, 22, 0, 0, 0),
                end_time: local_time(2026, 1, 2, 12, 0, 0, 0),
                repair: Some(StrBoundaryRepair::HistoricalTrailingNoon),
            }
        );
        assert!(
            index
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_str_repaired_boundaries")
        );
    }

    #[test]
    fn str_filename_ownership_is_half_open_and_never_expands_transitively() {
        let mut source = MemorySource::default();
        source.insert(
            "STR.edf",
            synthetic_str("serial-123", &[540, 660], &[660, 780]),
        );
        source.insert(
            "DATALOG/20260101_213000_BRP.edf",
            synthetic_edf("01.01.26", "21.30.00", 10_800, "1"),
        );
        source.insert(
            "DATALOG/20260101_230000_PLD.edf",
            synthetic_edf("01.01.26", "23.00.00", 60, "1"),
        );

        let index = index_session_candidates_for_machine(&source, "serial-123").expect("STR index");
        assert_eq!(index.candidates.len(), 2);
        assert_eq!(
            index.candidates[0]
                .files
                .iter()
                .map(|file| file.kind)
                .collect::<Vec<_>>(),
            [ResmedSessionFileKind::Brp]
        );
        assert_eq!(
            index.candidates[1]
                .files
                .iter()
                .map(|file| file.kind)
                .collect::<Vec<_>>(),
            [ResmedSessionFileKind::Pld]
        );
        let ResmedSessionAnchor::StrMask {
            mask_on_minute,
            mask_off_minute,
            ..
        } = index.candidates[0].anchor
        else {
            panic!("first candidate must remain STR anchored");
        };
        assert_eq!((mask_on_minute, mask_off_minute), (540, 660));
    }

    #[test]
    fn str_candidate_lookup_uses_logarithmic_day_bucket_probes() {
        let candidates: Vec<_> = (0..1_024)
            .map(|index| WorkingCandidate {
                interval_start_millis: i64::from(index) * 10,
                interval_end_millis: i64::from(index) * 10 + 5,
                resmed_day: "2026-01-01".to_owned(),
                anchor: ResmedSessionAnchor::DetailFallback,
                files: Vec::new(),
            })
            .collect();
        let indices: Vec<_> = (0..candidates.len()).collect();
        let (position, probes) = binary_candidate_start_counted(&indices, &candidates, 5_000, true);
        assert_eq!(position, 501);
        assert!(
            probes <= 11,
            "1,024 candidates should require at most 11 binary probes"
        );
    }

    #[test]
    fn binary_str_ownership_matches_forward_filename_then_reverse_overlap_reference() {
        fn candidate(day: &str, start: i64, end: i64) -> WorkingCandidate {
            WorkingCandidate {
                interval_start_millis: start,
                interval_end_millis: end,
                resmed_day: day.to_owned(),
                anchor: ResmedSessionAnchor::DetailFallback,
                files: Vec::new(),
            }
        }
        fn file(day: &str, filename: i64, selected: i64, end: Option<i64>) -> IndexedFile {
            let time = local_time(2026, 1, 1, 12, 0, 0, 0);
            IndexedFile {
                file: ResmedSessionFile {
                    relative_path: "DATALOG/reference_BRP.edf".to_owned(),
                    size_bytes: 512,
                    kind: ResmedSessionFileKind::Brp,
                    scope: ResmedSessionFileScope::Session,
                    filename_start_time: time.clone(),
                    edf_header: None,
                    selected_start_time: time,
                    timestamp_source: ResmedTimestampSource::Filename,
                },
                filename_millis: filename,
                selected_millis: selected,
                grouping_end_millis: end,
                estimated_end_millis: end,
                resmed_day: day.to_owned(),
            }
        }
        fn linear_reference(file: &IndexedFile, candidates: &[WorkingCandidate]) -> Option<usize> {
            if let Some(index) = candidates.iter().position(|candidate| {
                file.resmed_day == candidate.resmed_day
                    && file.filename_millis >= candidate.interval_start_millis
                    && file.filename_millis < candidate.interval_end_millis
            }) {
                return Some(index);
            }
            let file_end = file.grouping_end_millis?;
            candidates.iter().rposition(|candidate| {
                file.resmed_day == candidate.resmed_day
                    && candidate.interval_start_millis < file_end
                    && file.selected_millis < candidate.interval_end_millis
            })
        }

        let candidates = vec![
            candidate("2026-01-01", 100, 200),
            candidate("2026-01-01", 300, 400),
            candidate("2026-01-02", 100, 200),
        ];
        let mut by_day = BTreeMap::<String, Vec<usize>>::new();
        for (index, candidate) in candidates.iter().enumerate() {
            by_day
                .entry(candidate.resmed_day.clone())
                .or_default()
                .push(index);
        }
        let cases = [
            file("2026-01-01", 100, 100, Some(101)),
            file("2026-01-01", 199, 199, Some(400)),
            file("2026-01-01", 200, 200, None),
            file("2026-01-01", 300, 300, Some(301)),
            file("2026-01-01", 250, 150, Some(350)),
            file("2026-01-01", 250, 150, Some(300)),
            file("2026-01-01", 250, 200, Some(300)),
            file("2026-01-02", 150, 150, Some(151)),
            file("2026-01-03", 150, 150, Some(151)),
        ];
        for case in cases {
            assert_eq!(
                find_str_candidate_match(&case, &candidates, &by_day),
                linear_reference(&case, &candidates),
                "binary ownership diverged for {case:?}"
            );
        }
    }

    #[test]
    fn fallback_grouping_budget_is_global_exact_and_all_or_none() {
        fn fallback_file(
            path: &str,
            filename_millis: i64,
            selected_millis: i64,
            grouping_end_millis: Option<i64>,
        ) -> IndexedFile {
            let time = local_time(2026, 1, 1, 12, 0, 0, 0);
            IndexedFile {
                file: ResmedSessionFile {
                    relative_path: path.to_owned(),
                    size_bytes: 512,
                    kind: ResmedSessionFileKind::Brp,
                    scope: ResmedSessionFileScope::Session,
                    filename_start_time: time.clone(),
                    edf_header: None,
                    selected_start_time: time,
                    timestamp_source: ResmedTimestampSource::Filename,
                },
                filename_millis,
                selected_millis,
                grouping_end_millis,
                estimated_end_millis: grouping_end_millis,
                resmed_day: "2026-01-01".to_owned(),
            }
        }

        let files = vec![
            fallback_file("DATALOG/a_BRP.edf", 0, 0, Some(10)),
            fallback_file("DATALOG/b_BRP.edf", 15, 15, Some(15)),
            fallback_file("DATALOG/c_BRP.edf", 20, 20, Some(30)),
            fallback_file("DATALOG/d_BRP.edf", 5, 5, None),
        ];

        // a creates the first group without a comparison. The zero-duration b
        // and bounded c each consume one forward plus one reverse comparison;
        // unbounded d consumes the fifth comparison in the forward scan.
        let mut exact_warnings = Vec::new();
        let exact = group_session_files_with_limit(files.clone(), &mut exact_warnings, 5);
        assert_eq!(exact.len(), 2);
        assert_eq!(exact[0].files.len(), 2);
        assert_eq!(exact[1].files.len(), 1);
        assert_eq!(exact_warnings.len(), 1);
        assert_eq!(exact_warnings[0].code, "zero_duration_session_edf");
        let exact_manifest: Vec<_> = exact.into_iter().map(finalize_candidate).collect();

        let mut reference_warnings = Vec::new();
        let reference =
            group_session_files_with_limit(files.clone(), &mut reference_warnings, usize::MAX);
        let reference_manifest: Vec<_> = reference.into_iter().map(finalize_candidate).collect();
        assert_eq!(exact_manifest, reference_manifest);
        assert_eq!(exact_warnings, reference_warnings);

        let retained_str = WorkingCandidate {
            interval_start_millis: 100,
            interval_end_millis: 200,
            resmed_day: "2026-01-02".to_owned(),
            anchor: ResmedSessionAnchor::StrMask {
                therapy_day: "2026-01-02".to_owned(),
                mask_on_minute: 0,
                mask_off_minute: 1,
                source_mask_on_value: Some(0),
                source_mask_off_value: Some(1),
                start_time: local_time(2026, 1, 2, 12, 0, 0, 0),
                end_time: local_time(2026, 1, 2, 12, 1, 0, 0),
                repair: None,
            },
            files: Vec::new(),
        };
        let mut candidates = vec![retained_str];
        let mut exceeded_warnings = vec![warning(
            "existing_warning",
            "A diagnostic produced before fallback grouping",
            Some("DATALOG/existing.edf"),
        )];
        candidates.extend(group_session_files_with_limit(
            files,
            &mut exceeded_warnings,
            4,
        ));
        assert_eq!(candidates.len(), 1, "verified STR candidate is retained");
        assert!(matches!(
            candidates[0].anchor,
            ResmedSessionAnchor::StrMask { .. }
        ));
        assert_eq!(
            exceeded_warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            [
                "existing_warning",
                "resmed_fallback_grouping_limit_exceeded"
            ]
        );
        assert_eq!(exceeded_warnings[1].relative_path, None);
        assert!(
            exceeded_warnings[1]
                .message
                .contains("4-comparison work limit")
        );
    }

    #[test]
    fn daywide_attachment_is_all_or_none_at_checked_output_limit() {
        fn candidates() -> Vec<WorkingCandidate> {
            (0..2)
                .map(|index| WorkingCandidate {
                    interval_start_millis: i64::from(index) * 10,
                    interval_end_millis: i64::from(index) * 10 + 5,
                    resmed_day: "2026-01-01".to_owned(),
                    anchor: ResmedSessionAnchor::DetailFallback,
                    files: Vec::new(),
                })
                .collect()
        }
        fn day_file(path: &str) -> IndexedFile {
            let time = local_time(2026, 1, 1, 12, 0, 0, 0);
            IndexedFile {
                file: ResmedSessionFile {
                    relative_path: path.to_owned(),
                    size_bytes: 512,
                    kind: ResmedSessionFileKind::Eve,
                    scope: ResmedSessionFileScope::ResmedDay,
                    filename_start_time: time.clone(),
                    edf_header: None,
                    selected_start_time: time,
                    timestamp_source: ResmedTimestampSource::Filename,
                },
                filename_millis: 0,
                selected_millis: 0,
                grouping_end_millis: None,
                estimated_end_millis: None,
                resmed_day: "2026-01-01".to_owned(),
            }
        }
        let files = vec![
            day_file("DATALOG/20260101_120000_EVE.edf"),
            day_file("DATALOG/20260101_120001_EVE.edf"),
        ];

        let mut rejected = candidates();
        let mut warnings = Vec::new();
        attach_day_files_with_limit(&mut rejected, files.clone(), &mut warnings, 3);
        assert!(rejected.iter().all(|candidate| candidate.files.is_empty()));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.code == "resmed_daywide_association_limit_exceeded")
        );

        let mut accepted = candidates();
        let mut warnings = Vec::new();
        attach_day_files_with_limit(&mut accepted, files, &mut warnings, 4);
        assert!(warnings.is_empty());
        assert!(accepted.iter().all(|candidate| candidate.files.len() == 2));
    }

    #[test]
    fn mismatched_or_duplicate_str_preserves_exact_detail_fallback_candidates() {
        let mut mismatched = MemorySource::default();
        mismatched.insert("STR.edf", synthetic_str("other-card", &[600], &[660]));
        mismatched.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );
        let expected = index_session_candidates(&mismatched).expect("detail fallback reference");
        let actual = index_session_candidates_for_machine(&mismatched, "serial-123")
            .expect("mismatched STR soft fallback");
        assert_eq!(actual.candidates, expected.candidates);
        assert!(
            actual
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_str_serial_mismatch")
        );

        let mut duplicate = mismatched;
        duplicate.files.insert(
            "STR.edf".to_owned(),
            synthetic_str("serial-123", &[600, 600], &[630, 660]),
        );
        let entry = duplicate
            .entries
            .iter_mut()
            .find(|entry| entry.relative_path == "STR.edf")
            .expect("STR inventory entry");
        entry.size_bytes = u64::try_from(duplicate.files["STR.edf"].len()).expect("fixture size");
        let actual = index_session_candidates_for_machine(&duplicate, "serial-123")
            .expect("duplicate mask-on soft fallback");
        assert_eq!(actual.candidates, expected.candidates);
        assert!(
            actual
                .warnings
                .iter()
                .any(|warning| warning.code == "resmed_str_ambiguous_days")
        );
    }

    #[test]
    fn groups_flat_s9_and_nested_files_and_attaches_daywide_annotations() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_220000_PLD.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_220000_SAD.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_220000_SA2.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_120000_EVE.edf",
            synthetic_edf("01.01.26", "12.00.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_120000_CSL.edf",
            synthetic_edf("01.01.26", "12.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.warnings.is_empty());
        assert_eq!(index.candidates.len(), 1);
        let candidate = &index.candidates[0];
        assert_eq!(
            candidate.id,
            expected_id("20260101-220000", "20260101_220000_BRP.edf")
        );
        assert_eq!(candidate.resmed_day, "2026-01-01");
        assert_eq!(
            candidate
                .files
                .iter()
                .map(|file| file.kind)
                .collect::<Vec<_>>(),
            [
                ResmedSessionFileKind::Brp,
                ResmedSessionFileKind::Pld,
                ResmedSessionFileKind::Sa2,
                ResmedSessionFileKind::Sad,
                ResmedSessionFileKind::Csl,
                ResmedSessionFileKind::Eve,
            ]
        );
        assert!(
            candidate.files[4..]
                .iter()
                .all(|file| file.scope == ResmedSessionFileScope::ResmedDay)
        );
    }

    #[test]
    fn scanfiles_accepts_flat_and_one_year_or_date_level_datalog_layouts() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_130000_BRP.edf",
            synthetic_edf("01.01.26", "13.00.00", 60, "1"),
        );
        source.insert(
            "DATALOG/2026/20260101_140000_BRP.edf",
            synthetic_edf("01.01.26", "14.00.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_150000_BRP.edf",
            synthetic_edf("01.01.26", "15.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 3);
        assert!(index.warnings.is_empty());
    }

    #[test]
    fn groups_overlapping_header_intervals_and_orders_sessions() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101/20260101_230000_BRP.edf",
            synthetic_edf("01.01.26", "23.00.00", 300, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_220030_PLD.edf",
            synthetic_edf("01.01.26", "22.00.30", 300, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 2);
        assert_eq!(
            index.candidates[0].id,
            expected_id("20260101-220000", "20260101_220000_BRP.edf")
        );
        assert_eq!(index.candidates[0].files.len(), 2);
        assert_eq!(
            index.candidates[1].id,
            expected_id("20260101-230000", "20260101_230000_BRP.edf")
        );
    }

    #[test]
    fn filename_interval_hit_does_not_expand_the_grouping_envelope() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101_220500_PLD.edf",
            synthetic_edf("01.01.26", "22.15.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101_222000_BRP.edf",
            synthetic_edf("01.01.26", "22.20.00", 300, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 2);
        assert_eq!(index.candidates[0].files.len(), 2);
        assert_eq!(index.candidates[1].files.len(), 1);
        assert_eq!(
            index.candidates[0]
                .estimated_end_time
                .as_ref()
                .expect("estimated end")
                .wall_time,
            "2026-01-01T22:25:00",
            "candidate output still describes all attached files"
        );
    }

    #[test]
    fn filename_hits_scan_forward_after_reverse_overlap_expansion() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "23.00.00", 3_600, "1"),
        );
        source.insert(
            "DATALOG/20260101_221000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 1_800, "1"),
        );
        source.insert(
            "DATALOG/20260101_224000_PLD.edf",
            synthetic_edf("01.01.26", "22.20.00", 3_600, "1"),
        );
        source.insert(
            "DATALOG/20260101_231000_PLD.edf",
            synthetic_edf("01.01.26", "23.10.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 2);
        let first_created = index
            .candidates
            .iter()
            .find(|candidate| {
                candidate
                    .files
                    .iter()
                    .any(|file| file.relative_path.ends_with("220000_BRP.edf"))
            })
            .expect("first-created interval");
        assert!(
            first_created
                .files
                .iter()
                .any(|file| file.relative_path.ends_with("231000_PLD.edf")),
            "OSCAR's forward filename pass chooses the first matching interval"
        );
        let reverse_overlap_target = index
            .candidates
            .iter()
            .find(|candidate| {
                candidate
                    .files
                    .iter()
                    .any(|file| file.relative_path.ends_with("221000_BRP.edf"))
            })
            .expect("reverse-overlap target");
        assert!(
            reverse_overlap_target
                .files
                .iter()
                .any(|file| file.relative_path.ends_with("224000_PLD.edf")),
            "OSCAR's reverse duration pass chooses the last overlapping interval"
        );
    }

    #[test]
    fn rejects_corrupt_and_truncated_headers_without_false_candidates() {
        let mut source = MemorySource::default();
        source.insert("DATALOG/20260101_220000_BRP.edf", b"not an EDF".to_vec());
        let mut corrupt = synthetic_edf("01.01.26", "23.00.00", 60, "1");
        corrupt[184..192].copy_from_slice(b"99999999");
        source.insert("DATALOG/20260101_230000_BRP.edf", corrupt);

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.candidates.is_empty());
        assert_eq!(
            index
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            ["invalid_edf_header", "invalid_edf_header"]
        );
    }

    #[test]
    fn corrupt_header_datetime_retains_duration_and_falls_back_to_filename() {
        let mut source = MemorySource::default();
        let mut bytes = synthetic_edf("01.01.26", "22.00.00", 600, "1");
        bytes[168..184].copy_from_slice(b"xx.xx.xxxx.xx.xx");
        source.insert("DATALOG/20260101_220000_BRP.edf", bytes);

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        let candidate = &index.candidates[0];
        assert_eq!(candidate.start_time.wall_time, "2026-01-01T22:00:00");
        assert_eq!(
            candidate
                .estimated_end_time
                .as_ref()
                .expect("duration-derived end")
                .wall_time,
            "2026-01-01T22:10:00"
        );
        let file = &candidate.files[0];
        assert_eq!(file.timestamp_source, ResmedTimestampSource::Filename);
        assert!(
            file.edf_header
                .as_ref()
                .expect("duration header")
                .start_time
                .is_none()
        );
        assert_eq!(index.warnings[0].code, "invalid_edf_header_time");
    }

    #[test]
    fn annotation_only_files_follow_scanfiles_without_header_parsing() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101_120000_EVE.edf",
            b"annotation payload is parsed only during import".to_vec(),
        );
        source.insert(
            "DATALOG/20260101_120000_CSL.edf",
            b"CSR start/end payload is parsed only during import".to_vec(),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.warnings.is_empty());
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(index.candidates[0].files.len(), 3);
        assert!(
            index.candidates[0]
                .files
                .iter()
                .filter(|file| file.scope == ResmedSessionFileScope::ResmedDay)
                .all(|file| file.edf_header.is_none())
        );
    }

    #[test]
    fn empty_edf_is_skipped_before_header_or_annotation_handling() {
        let mut source = MemorySource::default();
        source.insert("DATALOG/20260101_120000_EVE.edf", Vec::new());

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.candidates.is_empty());
        assert_eq!(index.warnings[0].code, "empty_resmed_edf");
    }

    #[test]
    fn filename_wins_when_valid_header_drifts_more_than_six_hours() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "08.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        let file = &index.candidates[0].files[0];
        assert_eq!(
            file.edf_header
                .as_ref()
                .and_then(|header| header.start_time.as_ref())
                .map(|time| time.wall_time.as_str()),
            Some("2026-01-01T08:00:00")
        );
        assert_eq!(file.selected_start_time.wall_time, "2026-01-01T22:00:00");
        assert_eq!(file.timestamp_source, ResmedTimestampSource::Filename);
        assert_eq!(index.warnings[0].code, "edf_header_filename_drift");
        assert_eq!(index.warnings[0].session_id, None);
    }

    #[test]
    fn supplied_current_time_makes_future_header_fall_back_to_filename() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260102_200000_BRP.edf",
            synthetic_edf("02.01.26", "21.30.00", 60, "1"),
        );

        let current_device_local_time = local_time(2026, 1, 2, 21, 0, 0, 0);
        let index = super::index_session_candidates(&source, &current_device_local_time)
            .expect("candidate index");
        let file = &index.candidates[0].files[0];
        assert_eq!(
            file.edf_header
                .as_ref()
                .and_then(|header| header.start_time.as_ref())
                .map(|time| time.wall_time.as_str()),
            Some("2026-01-02T21:30:00")
        );
        assert_eq!(file.selected_start_time.wall_time, "2026-01-02T20:00:00");
        assert_eq!(file.timestamp_source, ResmedTimestampSource::Filename);
        assert_eq!(index.warnings[0].code, "edf_header_time_in_future");
    }

    #[test]
    fn pre_2000_edf_pivot_year_receives_oscars_century_repair() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20850101_220000_BRP.edf",
            synthetic_edf("01.01.85", "22.00.00", 60, "1"),
        );

        let current_device_local_time = local_time(2090, 1, 1, 0, 0, 0, 0);
        let index = super::index_session_candidates(&source, &current_device_local_time)
            .expect("candidate index");
        let file = &index.candidates[0].files[0];
        assert_eq!(file.timestamp_source, ResmedTimestampSource::EdfHeader);
        assert_eq!(file.selected_start_time.wall_time, "2085-01-01T22:00:00");
        assert!(index.warnings.is_empty());
    }

    #[test]
    fn duplicate_basename_prefers_flat_s9_layout_deterministically() {
        let mut source = MemorySource::default();
        let bytes = synthetic_edf("01.01.26", "22.00.00", 60, "1");
        source.insert("DATALOG/2026/20260101_220000_BRP.edf", bytes.clone());
        source.insert("DATALOG/20260101_220000_BRP.edf", bytes);

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(
            index.candidates[0].files[0].relative_path,
            "DATALOG/20260101_220000_BRP.edf"
        );
        assert_eq!(index.warnings.len(), 1);
        assert_eq!(index.warnings[0].code, "duplicate_resmed_edf");
        assert_eq!(
            index.warnings[0].relative_path.as_deref(),
            Some("DATALOG/2026/20260101_220000_BRP.edf")
        );
    }

    #[test]
    fn case_distinct_basenames_are_not_deduplicated() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260101_220000_brp.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(index.candidates[0].files.len(), 2);
        assert!(index.warnings.is_empty());
    }

    #[test]
    fn empty_flat_duplicate_does_not_hide_valid_nested_copy() {
        let mut source = MemorySource::default();
        source.insert("DATALOG/20260101_220000_BRP.edf", Vec::new());
        source.insert(
            "DATALOG/2026/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(
            index.candidates[0].files[0].relative_path,
            "DATALOG/2026/20260101_220000_BRP.edf"
        );
        assert_eq!(index.warnings.len(), 1);
        assert_eq!(index.warnings[0].code, "empty_resmed_edf");
    }

    #[test]
    fn unreadable_flat_duplicate_falls_through_to_valid_nested_copy() {
        let mut source = MemorySource::default();
        source.entries.push(SourceEntry {
            relative_path: "DATALOG/20260101_220000_BRP.edf".to_owned(),
            kind: SourceEntryKind::File,
            size_bytes: 512,
        });
        source.insert(
            "DATALOG/2026/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(
            index.candidates[0].files[0].relative_path,
            "DATALOG/2026/20260101_220000_BRP.edf"
        );
        assert_eq!(index.warnings.len(), 1);
        assert_eq!(index.warnings[0].code, "edf_header_read_failed");
    }

    #[test]
    fn readable_malformed_duplicate_claims_basename_before_duration_scan() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            b"readable but not an EDF".to_vec(),
        );
        source.insert(
            "DATALOG/2026/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.candidates.is_empty());
        assert_eq!(
            index
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            ["duplicate_resmed_edf", "invalid_edf_header"]
        );
    }

    #[test]
    fn zero_duration_file_attaches_by_filename_but_cannot_create_a_session() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101_220000_PLD.edf",
            synthetic_edf("01.01.26", "22.00.00", 0, "1"),
        );
        source.insert(
            "DATALOG/20260101_230000_BRP.edf",
            synthetic_edf("01.01.26", "23.00.00", 0, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(index.candidates[0].files.len(), 2);
        assert_eq!(index.warnings.len(), 1);
        assert_eq!(index.warnings[0].code, "zero_duration_session_edf");
        assert_eq!(
            index.warnings[0].relative_path.as_deref(),
            Some("DATALOG/20260101_230000_BRP.edf")
        );
    }

    #[test]
    fn fractional_duration_is_truncated_to_oscars_whole_seconds() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf_with_record_duration("01.01.26", "22.00.00", "60.9", "1"),
        );
        source.insert(
            "DATALOG/20260101_220100_PLD.edf",
            synthetic_edf("01.01.26", "22.01.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260101_230000_BRP.edf",
            synthetic_edf_with_record_duration("01.01.26", "23.00.00", "0.9", "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 2);
        assert_eq!(
            index.candidates[0].files[0]
                .edf_header
                .as_ref()
                .expect("header")
                .estimated_duration_millis,
            Some(60_000)
        );
        assert_eq!(index.warnings.len(), 1);
        assert_eq!(index.warnings[0].code, "zero_duration_session_edf");
    }

    #[test]
    fn before_noon_files_belong_to_previous_resmed_day() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260102/20260102_013000_BRP.edf",
            synthetic_edf("02.01.26", "01.30.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_120000_EVE.edf",
            synthetic_edf("01.01.26", "12.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates[0].resmed_day, "2026-01-01");
        assert_eq!(index.candidates[0].files.len(), 2);
    }

    #[test]
    fn resmed_day_carries_scanfiles_filename_bucket_across_header_shift() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260102_130000_BRP.edf",
            synthetic_edf("02.01.26", "11.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        let candidate = &index.candidates[0];
        assert_eq!(candidate.start_time.wall_time, "2026-01-02T11:00:00");
        assert_eq!(candidate.resmed_day, "2026-01-02");
        assert_eq!(
            resmed_day(&candidate.start_time),
            "2026-01-01",
            "the selected header time would produce a different noon bucket"
        );
    }

    #[test]
    fn daywide_file_without_session_is_reported_not_promoted() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_120000_EVE.edf",
            synthetic_edf("01.01.26", "12.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.candidates.is_empty());
        assert_eq!(index.warnings[0].code, "daywide_edf_without_session");
    }

    #[test]
    fn compressed_and_malformed_names_are_explicitly_ignored() {
        let mut source = MemorySource::default();
        source.insert("DATALOG/20260101_220000_BRP.edf.gz", vec![1, 2, 3]);
        source.insert("DATALOG/not-a-session.edf", vec![1, 2, 3]);
        source.insert("DATALOG/aéééb_220000_BRP.edf", vec![1, 2, 3]);

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.candidates.is_empty());
        assert_eq!(
            index
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            [
                "compressed_edf_not_indexed",
                "invalid_resmed_edf_filename",
                "invalid_resmed_edf_filename"
            ]
        );
    }

    #[test]
    fn rejects_huge_duration_without_constructing_an_out_of_range_end_time() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 90_000_000, "99999999"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.candidates.is_empty());
        assert_eq!(index.warnings.len(), 1);
        assert_eq!(index.warnings[0].code, "edf_duration_out_of_range");
    }

    #[test]
    fn candidate_ids_include_source_filename_identity_to_avoid_local_time_collisions() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260102/20260102_110000_BRP.edf",
            synthetic_edf("02.01.26", "11.30.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260102/20260102_120000_BRP.edf",
            synthetic_edf("02.01.26", "11.30.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 2);
        assert_eq!(
            index.candidates[0].start_time,
            index.candidates[1].start_time
        );
        assert_ne!(index.candidates[0].id, index.candidates[1].id);
        assert_eq!(
            index.candidates[0].id,
            expected_id("20260102-113000", "20260102_110000_BRP.edf")
        );
        assert_eq!(
            index.candidates[1].id,
            expected_id("20260102-113000", "20260102_120000_BRP.edf")
        );
    }

    #[test]
    fn overlap_fallback_groups_same_filename_time_without_a_generic_lag_guard() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "23.00.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260101_220000_PLD.edf",
            synthetic_edf("01.01.26", "23.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(index.candidates[0].files.len(), 2);
    }

    #[test]
    fn scanfiles_layout_rejects_archive_and_deep_datalog_directories() {
        let mut source = MemorySource::default();
        let bytes = synthetic_edf("01.01.26", "22.00.00", 60, "1");
        source.insert("DATALOG/archive/20260101_220000_BRP.edf", bytes.clone());
        source.insert("DATALOG/2026/01/20260101_220000_PLD.edf", bytes.clone());
        source.insert("DATALOG/20261340/20260101_220000_SAD.edf", bytes);

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.candidates.is_empty());
        assert_eq!(index.warnings.len(), 3);
        assert!(
            index
                .warnings
                .iter()
                .all(|warning| warning.code == "unsupported_resmed_datalog_layout")
        );
    }

    #[test]
    fn unknown_record_count_attaches_only_to_an_established_candidate() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101_220000_PLD.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "-1"),
        );
        source.insert(
            "DATALOG/20260101_230000_BRP.edf",
            synthetic_edf("01.01.26", "23.00.00", 600, "-1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(index.candidates[0].files.len(), 2);
        assert!(
            index.candidates[0]
                .files
                .iter()
                .any(|file| file.relative_path.ends_with("_PLD.edf"))
        );
        assert!(
            !index.candidates[0]
                .files
                .iter()
                .any(|file| file.relative_path.ends_with("230000_BRP.edf"))
        );
        assert_eq!(
            index
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            [
                "edf_record_count_unknown",
                "edf_record_count_unknown",
                "unbounded_session_edf_not_indexed"
            ]
        );
    }

    #[test]
    fn unknown_record_count_can_overlap_as_selected_header_point() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101_230000_PLD.edf",
            synthetic_edf("01.01.26", "22.05.00", 60, "-1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(index.candidates[0].files.len(), 2);
        assert_eq!(index.warnings.len(), 1);
        assert_eq!(index.warnings[0].code, "edf_record_count_unknown");
        assert!(
            index.candidates[0]
                .files
                .iter()
                .find(|file| file.kind == ResmedSessionFileKind::Pld)
                .expect("unknown-count file")
                .edf_header
                .as_ref()
                .expect("header")
                .estimated_duration_millis
                .is_none()
        );
    }

    #[test]
    fn six_hour_drift_boundary_matches_pinned_get_edf_duration() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "16.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert!(index.warnings.is_empty());
        let file = &index.candidates[0].files[0];
        assert_eq!(file.timestamp_source, ResmedTimestampSource::EdfHeader);
        assert_eq!(file.selected_start_time.wall_time, "2026-01-01T16:00:00");
    }

    #[test]
    fn overlap_fallback_has_no_sql_only_ten_minute_filename_lag_cutoff() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_214000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 600, "1"),
        );
        source.insert(
            "DATALOG/20260101_214959_PLD.edf",
            synthetic_edf("01.01.26", "22.05.00", 600, "1"),
        );
        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(
            index
                .candidates
                .iter()
                .filter(|candidate| candidate.resmed_day == "2026-01-01")
                .count(),
            1,
            "overlap alone groups the files even though the second filename is more than ten minutes before the selected interval"
        );
    }

    #[test]
    fn shuffled_inventory_produces_the_same_manifest() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101/20260101_220000_PLD.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260101_220000_BRP.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );
        source.insert(
            "DATALOG/20260101/20260101_120000_EVE.edf",
            synthetic_edf("01.01.26", "12.00.00", 60, "1"),
        );
        let mut shuffled = source.clone();
        shuffled.entries.reverse();

        assert_eq!(
            index_session_candidates(&source).expect("ordered index"),
            index_session_candidates(&shuffled).expect("shuffled index")
        );
    }

    #[test]
    fn portable_inventory_entry_budget_is_enforced_before_file_reads() {
        let source = MemorySource::default();
        let entry = SourceEntry {
            relative_path: "DATALOG/20260101_220000_BRP.edf".to_owned(),
            kind: SourceEntryKind::File,
            size_bytes: 512,
        };
        let inventory = SourceInventory {
            entries: vec![entry; RESMED_SESSION_INDEX_MAX_ENTRIES + 1],
            total_file_bytes: 0,
        };

        let error =
            index_session_candidates_from_inventory(&source, &inventory).expect_err("entry limit");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);
    }

    #[test]
    fn invalid_supplied_current_time_is_rejected() {
        let source = MemorySource::default();
        let invalid = ResmedDeviceLocalTime {
            wall_time: "invalid".to_owned(),
            year: 2026,
            month: 13,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
        };

        let error = super::index_session_candidates(&source, &invalid)
            .expect_err("invalid current time must fail");
        assert_eq!(error.kind, ImportErrorKind::InvalidData);
    }

    #[test]
    fn portable_inventory_path_budget_is_enforced_before_file_reads() {
        let source = MemorySource::default();
        let inventory = SourceInventory {
            entries: vec![SourceEntry {
                relative_path: "x".repeat(RESMED_SESSION_INDEX_MAX_PATH_BYTES + 1),
                kind: SourceEntryKind::File,
                size_bytes: 0,
            }],
            total_file_bytes: 0,
        };

        let error =
            index_session_candidates_from_inventory(&source, &inventory).expect_err("path limit");
        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);
    }

    #[test]
    fn aev_groups_but_payload_decoding_remains_explicitly_unsupported() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_AEV.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(
            index.candidates[0].files[0].kind,
            ResmedSessionFileKind::Aev
        );
        assert_eq!(index.warnings[0].code, "resmed_aev_payload_not_decoded");
    }

    #[test]
    fn unknown_edf_type_groups_but_payload_decoding_is_explicitly_unsupported() {
        let mut source = MemorySource::default();
        source.insert(
            "DATALOG/20260101_220000_VENDOR.edf",
            synthetic_edf("01.01.26", "22.00.00", 60, "1"),
        );

        let index = index_session_candidates(&source).expect("candidate index");
        assert_eq!(index.candidates.len(), 1);
        assert_eq!(
            index.candidates[0].files[0].kind,
            ResmedSessionFileKind::Unknown
        );
        assert_eq!(index.warnings.len(), 1);
        assert_eq!(
            index.warnings[0].code,
            "unknown_resmed_edf_payload_not_decoded"
        );
        assert!(index.warnings[0].message.contains("candidate grouping"));
    }

    #[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
    #[test]
    fn native_index_reads_only_a_bounded_prefix_of_large_waveform_files() {
        use crate::DirectorySource;
        use std::fs;
        use tempfile::TempDir;

        let root = TempDir::new().expect("temporary card");
        fs::create_dir(root.path().join("DATALOG")).expect("DATALOG");
        let mut bytes = synthetic_edf("01.01.26", "22.00.00", 60, "1");
        bytes.resize(RESMED_EDF_HEADER_MAX_BYTES + 1_024, 0);
        let path = "DATALOG/20260101_220000_BRP.edf";
        fs::write(root.path().join(path), bytes).expect("large waveform fixture");
        let source = DirectorySource::open(root.path()).expect("directory source");

        assert!(
            source.read_file(path, RESMED_EDF_HEADER_MAX_BYTES).is_err(),
            "complete-file reads remain size limited"
        );
        let index = index_session_candidates(&source).expect("header-only index");
        assert_eq!(index.candidates.len(), 1);
        assert!(index.warnings.is_empty());
    }

    fn synthetic_str(serial: &str, mask_on: &[i16], mask_off: &[i16]) -> Vec<u8> {
        assert_eq!(mask_on.len(), mask_off.len());
        assert!(!mask_on.is_empty());
        let signal_labels = ["Mask On", "Mask Off", "Mask Events"];
        let samples_per_record = [mask_on.len(), mask_off.len(), 1];
        let header_bytes = 256 + signal_labels.len() * 256;
        let mut bytes = Vec::new();
        for (value, width) in [
            ("0".to_owned(), 8),
            ("patient".to_owned(), 80),
            (format!("ResMed SRN={serial}"), 80),
            ("01.01.2612.00.00".to_owned(), 16),
            (header_bytes.to_string(), 8),
            (String::new(), 44),
            ("1".to_owned(), 8),
            ("86400".to_owned(), 8),
            (signal_labels.len().to_string(), 4),
        ] {
            bytes.extend(padded_field(&value, width));
        }
        for label in signal_labels {
            bytes.extend(padded_field(label, 16));
        }
        for _ in signal_labels {
            bytes.extend(padded_field("", 80));
        }
        for _ in signal_labels {
            bytes.extend(padded_field("raw", 8));
        }
        for _ in signal_labels {
            bytes.extend(padded_field("-32768", 8));
        }
        for _ in signal_labels {
            bytes.extend(padded_field("32767", 8));
        }
        for _ in signal_labels {
            bytes.extend(padded_field("-32768", 8));
        }
        for _ in signal_labels {
            bytes.extend(padded_field("32767", 8));
        }
        for _ in signal_labels {
            bytes.extend(padded_field("", 80));
        }
        for count in samples_per_record {
            bytes.extend(padded_field(&count.to_string(), 8));
        }
        for _ in signal_labels {
            bytes.extend(padded_field("", 32));
        }
        assert_eq!(bytes.len(), header_bytes);
        for value in mask_on {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in mask_off {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let event_count = i16::try_from(mask_on.len() * 2).expect("fixture event count");
        bytes.extend_from_slice(&event_count.to_le_bytes());
        bytes
    }

    fn padded_field(value: &str, width: usize) -> Vec<u8> {
        assert!(value.len() <= width);
        let mut bytes = vec![b' '; width];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        bytes
    }

    fn synthetic_edf(
        date: &str,
        time: &str,
        record_duration_seconds: u32,
        record_count: &str,
    ) -> Vec<u8> {
        synthetic_edf_with_record_duration(
            date,
            time,
            &record_duration_seconds.to_string(),
            record_count,
        )
    }

    fn synthetic_edf_with_record_duration(
        date: &str,
        time: &str,
        record_duration_seconds: &str,
        record_count: &str,
    ) -> Vec<u8> {
        let mut bytes = vec![b' '; 512];
        write_field(&mut bytes, 0, 8, "0");
        write_field(&mut bytes, 8, 80, "synthetic patient");
        write_field(&mut bytes, 88, 80, "synthetic recording");
        write_field(&mut bytes, 168, 8, date);
        write_field(&mut bytes, 176, 8, time);
        write_field(&mut bytes, 184, 8, "512");
        write_field(&mut bytes, 192, 44, "");
        write_field(&mut bytes, 236, 8, record_count);
        write_field(&mut bytes, 244, 8, record_duration_seconds);
        write_field(&mut bytes, 252, 4, "1");

        let mut offset = 256;
        for (width, value) in [
            (16, "Flow"),
            (80, ""),
            (8, "L/s"),
            (8, "-1"),
            (8, "1"),
            (8, "-32768"),
            (8, "32767"),
            (80, ""),
            (8, "1"),
            (32, ""),
        ] {
            write_field(&mut bytes, offset, width, value);
            offset += width;
        }
        bytes
    }

    fn write_field(bytes: &mut [u8], offset: usize, width: usize, value: &str) {
        assert!(value.len() <= width);
        bytes[offset..offset + value.len()].copy_from_slice(value.as_bytes());
    }

    fn expected_id(start: &str, basename: &str) -> String {
        format!(
            "resmed-local-{start}-file-{}",
            hex_identifier_component(basename)
        )
    }
}
