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
// oscar/SleepLib/loader_plugins/resmed_loader.h
// Modified: 2026-07-23

//! ResMed card detection, identification parsing, source inventory, and
//! bounded session-candidate indexing.
//!
//! The primary behavioral reference is OSCAR-code `resmed_loader.cpp` at commit
//! `64c5e90a26f91fb15868bcfcccde0c1e1522ac86`.
//!
//! OPAP deliberately corrects OSCAR's narrow product-family inference while
//! preserving source identity strings wherever [`MachineInfo`] has a raw field.
//! It also leaves an absent family empty instead of inheriting OSCAR's `S9`
//! default. See the identification parser comments and `opap_correction_*`
//! tests for the exact boundary.
//! Identification reads are additionally capped at 64 KiB. Legacy TGT input
//! must be UTF-8, uses literal `#KEY value` records, trims outer value
//! whitespace, and ignores key-only records rather than clearing an earlier
//! value. These are intentional bounded-input hardening differences from Qt's
//! permissive text reader and `QString::section` behavior.
//!
//! Detection here mirrors only OSCAR's lightweight `Detect` signature check.
//! It is not OSCAR `Open` import readiness: the latter also normalizes a
//! selected `DATALOG` path to its card root, accepts `STR.edf.gz`, requires a
//! parsed identification with a non-empty serial, and attempts to use only STR
//! summary data that parses and matches that serial. A bad STR can be omitted
//! while OSCAR continues with DATALOG detail files, so STR verification is not
//! itself an unconditional `Open` rejection gate. OPAP still does not import
//! STR summaries; the current detail slice emits BRP-backed partial sessions
//! from validated, uncompressed BRP waveforms and attaches trustworthy
//! uncompressed SAD/SA2 oximetry without treating it as therapy usage.

use crate::domain::{DeviceInfo, ImportWarning, WarningSeverity};
use crate::importer::{
    DeviceDiscovery, ImportError, ImportErrorKind, ImportOptions, ImportSource, Importer,
    SourceEntryKind, SourceInventory,
};
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
use crate::importer::{DirectorySource, SourceEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
use std::{
    fmt, io,
    path::{Path, PathBuf},
};

const DATALOG: &str = "DATALOG";
const STR_EDF: &str = "STR.edf";
const IDENT_TGT: &str = "Identification.tgt";
const IDENT_JSON: &str = "Identification.json";

mod csl;
mod eve;
mod pld;
mod session_import;
mod session_index;
pub mod str;
mod str_settings;
mod str_summary;

pub use session_import::{
    RESMED_BRP_MAX_FILE_BYTES, RESMED_BRP_MAX_FILES_PER_IMPORT,
    RESMED_BRP_MAX_OUTPUT_SAMPLES_PER_IMPORT, RESMED_BRP_MAX_TOTAL_BYTES_PER_IMPORT,
};
pub use session_index::{
    RESMED_EDF_HEADER_MAX_BYTES, RESMED_SESSION_INDEX_MAX_ENTRIES,
    RESMED_SESSION_INDEX_MAX_PATH_BYTES, RESMED_SESSION_INDEX_SCHEMA_VERSION,
    ResmedDeviceLocalTime, ResmedEdfHeaderSummary, ResmedSessionCandidate, ResmedSessionFile,
    ResmedSessionFileKind, ResmedSessionFileScope, ResmedSessionIndex, ResmedTimestampSource,
    index_session_candidates, index_session_candidates_from_inventory,
};

/// Stable identifier used by the ResMed importer.
pub const IMPORTER_ID: &str = "resmed";

/// Maximum accepted size of either ResMed identification file.
pub const IDENTIFICATION_MAX_BYTES: usize = 64 * 1024;

/// Backward-compatible export of the shared machine identity type.
pub use crate::domain::MachineInfo;

/// Semantic role assigned to a file found on a ResMed card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardFileRole {
    /// `Identification.json` or `Identification.tgt`.
    Identification,
    /// Root-level `STR.edf` summary and settings data.
    Summary,
    /// High-resolution `_BRP.edf` signal data.
    Waveform,
    /// Event annotations (`EVE`), Cheyne-Stokes respiration intervals (`CSL`),
    /// or currently unsupported `AEV` annotations.
    Events,
    /// Other recognized EDF detail data, including PLD and oximetry.
    Detail,
    /// Manufacturer checksum sidecar.
    Checksum,
    /// A file not currently interpreted by the importer.
    Other,
}

/// One file in a classified ResMed card inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardFile {
    /// Forward-slash-separated path relative to the card root.
    pub relative_path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Role inferred from the manufacturer filename convention.
    pub role: CardFileRole,
}

/// Deterministic file inventory for a detected ResMed card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardInventory {
    /// Files sorted by normalized relative path.
    pub files: Vec<CardFile>,
    /// Sum of all file sizes.
    pub total_bytes: u64,
}

/// Machine identity and file inventory discovered from native card media.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDiscovery {
    /// Identity parsed from the preferred identification file.
    pub machine: MachineInfo,
    /// Classified files available on the card.
    pub inventory: CardInventory,
}

/// Filesystem-independent ResMed importer.
///
/// Discovery, inventory, bounded EDF header indexing, and an intentionally
/// partial uncompressed BRP plus SAD/SA2 detail import are implemented.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResmedImporter;

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
#[derive(Debug)]
pub enum Error {
    NotResmedCard(PathBuf),
    MissingIdentification(PathBuf),
    InvalidIdentificationJson(&'static str),
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    Import {
        path: PathBuf,
        source: ImportError,
    },
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotResmedCard(path) => write!(formatter, "not a ResMed card: {}", path.display()),
            Self::MissingIdentification(path) => {
                write!(
                    formatter,
                    "no ResMed identification file in {}",
                    path.display()
                )
            }
            Self::InvalidIdentificationJson(field) => {
                write!(
                    formatter,
                    "missing or invalid Identification.json field: {field}"
                )
            }
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::Json { path, source } => {
                write!(formatter, "failed to parse {}: {source}", path.display())
            }
            Self::Import { path, source } => {
                write!(formatter, "failed to import {}: {source}", path.display())
            }
        }
    }
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Import { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Applies OSCAR-code `Detect`'s root signature rule with bounded, no-follow
/// native access: the selected directory must contain literal `DATALOG/` and
/// an uncompressed `STR.edf`.
///
/// This is a signature check, not import readiness. In particular it does not
/// parse identification, require a serial, attempt OSCAR's verified-STR summary
/// path, or accept a selected `DATALOG/` path or `STR.edf.gz` as `Open` does.
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
pub fn detect_card(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    DirectorySource::open(path).is_ok_and(|source| {
        source
            .has_direct_entry(DATALOG, SourceEntryKind::Directory)
            .is_ok_and(|present| present)
            && source
                .has_direct_entry(STR_EDF, SourceEntryKind::File)
                .is_ok_and(|present| present)
    })
}

/// Reads machine metadata, preferring `Identification.json` when both it and
/// the legacy TGT file are present, matching the pinned OSCAR-code loader.
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
pub fn read_machine_info(path: impl AsRef<Path>) -> Result<MachineInfo, Error> {
    let path = path.as_ref();
    let source = open_native_source(path)?;
    let inventory = native_identity_inventory(path, &source)?;
    if !is_resmed_inventory(&inventory) {
        return Err(Error::NotResmedCard(path.to_owned()));
    }
    read_machine_from_source(&source, &inventory)
        .map_err(|source| Error::Import {
            path: path.to_owned(),
            source,
        })?
        .ok_or_else(|| Error::MissingIdentification(path.to_owned()))
}

/// Recursively inventories and classifies files on a native ResMed card.
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
pub fn inventory_card(path: impl AsRef<Path>) -> Result<CardInventory, Error> {
    let path = path.as_ref();
    let source = open_native_source(path)?;
    let inventory = native_inventory(path, &source)?;
    if !is_resmed_inventory(&inventory) {
        return Err(Error::NotResmedCard(path.to_owned()));
    }
    Ok(classify_inventory(&inventory))
}

/// Discovers machine identity and classified files from a native ResMed card.
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
pub fn discover_card(path: impl AsRef<Path>) -> Result<CardDiscovery, Error> {
    let path = path.as_ref();
    let source = open_native_source(path)?;
    let inventory = native_inventory(path, &source)?;
    if !is_resmed_inventory(&inventory) {
        return Err(Error::NotResmedCard(path.to_owned()));
    }
    let machine = read_machine_from_source(&source, &inventory)
        .map_err(|source| Error::Import {
            path: path.to_owned(),
            source,
        })?
        .ok_or_else(|| Error::MissingIdentification(path.to_owned()))?;
    Ok(CardDiscovery {
        machine,
        inventory: classify_inventory(&inventory),
    })
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
fn open_native_source(path: &Path) -> Result<DirectorySource, Error> {
    DirectorySource::open(path).map_err(|source| Error::Import {
        path: path.to_owned(),
        source,
    })
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
fn native_inventory(path: &Path, source: &DirectorySource) -> Result<SourceInventory, Error> {
    source.inventory().map_err(|source| Error::Import {
        path: path.to_owned(),
        source,
    })
}

#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
fn native_identity_inventory(
    path: &Path,
    source: &DirectorySource,
) -> Result<SourceInventory, Error> {
    let mut entries = Vec::new();
    for (name, kind) in [
        (DATALOG, SourceEntryKind::Directory),
        (STR_EDF, SourceEntryKind::File),
        (IDENT_JSON, SourceEntryKind::File),
        (IDENT_TGT, SourceEntryKind::File),
    ] {
        let present = source
            .has_direct_entry(name, kind)
            .map_err(|source| Error::Import {
                path: path.to_owned(),
                source,
            })?;
        if present {
            entries.push(SourceEntry {
                relative_path: name.to_owned(),
                kind,
                size_bytes: 0,
            });
        }
    }
    Ok(SourceInventory {
        entries,
        total_file_bytes: 0,
    })
}

impl ResmedImporter {
    /// Classifies an already enumerated source without reading file contents.
    #[must_use]
    pub fn classify(inventory: &SourceInventory) -> CardInventory {
        classify_inventory(inventory)
    }
}

impl Importer for ResmedImporter {
    fn id(&self) -> &'static str {
        IMPORTER_ID
    }

    fn discover(&self, source: &dyn ImportSource) -> Result<Option<DeviceDiscovery>, ImportError> {
        let inventory = source.inventory()?;
        if !is_resmed_inventory(&inventory) {
            return Ok(None);
        }

        let mut warnings = Vec::new();
        let machine = if let Some(machine) = read_machine_from_source(source, &inventory)? {
            machine
        } else {
            warnings.push(ImportWarning {
                code: "missing_identification".to_owned(),
                severity: WarningSeverity::Warning,
                message: "The card has no ResMed identification file".to_owned(),
                relative_path: None,
                session_id: None,
            });
            MachineInfo::default()
        };

        Ok(Some(DeviceDiscovery {
            device: DeviceInfo {
                importer_id: IMPORTER_ID.to_owned(),
                machine,
            },
            inventory,
            warnings,
        }))
    }

    fn import(
        &self,
        source: &dyn ImportSource,
        options: &ImportOptions,
    ) -> Result<crate::domain::ImportReport, ImportError> {
        session_import::import_resmed_sessions(source, options)
    }
}

fn is_resmed_inventory(inventory: &SourceInventory) -> bool {
    inventory.has_directory(DATALOG) && inventory.has_file(STR_EDF)
}

fn classify_inventory(inventory: &SourceInventory) -> CardInventory {
    let mut files: Vec<_> = inventory
        .entries
        .iter()
        .filter(|entry| entry.kind == SourceEntryKind::File)
        .map(|entry| CardFile {
            relative_path: entry.relative_path.clone(),
            size_bytes: entry.size_bytes,
            role: role_for_path(&entry.relative_path),
        })
        .collect();
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let total_bytes = files.iter().map(|file| file.size_bytes).sum();

    CardInventory { files, total_bytes }
}

fn role_for_path(relative_path: &str) -> CardFileRole {
    if relative_path == IDENT_JSON || relative_path == IDENT_TGT {
        return CardFileRole::Identification;
    }
    if relative_path == STR_EDF {
        return CardFileRole::Summary;
    }

    let upper = relative_path.to_ascii_uppercase();
    if upper.ends_with(".CRC") {
        return CardFileRole::Checksum;
    }

    let edf_path = upper.strip_suffix(".GZ").unwrap_or(&upper);
    if edf_path.ends_with("_BRP.EDF") {
        CardFileRole::Waveform
    } else if ["_EVE.EDF", "_CSL.EDF", "_AEV.EDF"]
        .iter()
        .any(|suffix| edf_path.ends_with(suffix))
    {
        CardFileRole::Events
    } else if edf_path.ends_with(".EDF") {
        CardFileRole::Detail
    } else {
        CardFileRole::Other
    }
}

#[derive(Debug)]
enum JsonIdentificationError {
    Json(serde_json::Error),
    MissingField(&'static str),
}

fn read_machine_from_source(
    source: &dyn ImportSource,
    inventory: &SourceInventory,
) -> Result<Option<MachineInfo>, ImportError> {
    if inventory.has_file(IDENT_JSON) {
        let bytes = read_identification(source, IDENT_JSON)?;
        return parse_json_bytes(&bytes)
            .map(Some)
            .map_err(|error| match error {
                JsonIdentificationError::Json(source) => ImportError::new(
                    ImportErrorKind::InvalidData,
                    format!("failed to parse {IDENT_JSON}: {source}"),
                )
                .at_path(IDENT_JSON),
                JsonIdentificationError::MissingField(field) => ImportError::new(
                    ImportErrorKind::InvalidData,
                    format!("missing or invalid {IDENT_JSON} field: {field}"),
                )
                .at_path(IDENT_JSON),
            });
    }
    if inventory.has_file(IDENT_TGT) {
        let bytes = read_identification(source, IDENT_TGT)?;
        let text = std::str::from_utf8(&bytes).map_err(|source| {
            ImportError::new(
                ImportErrorKind::InvalidData,
                format!("{IDENT_TGT} is not UTF-8: {source}"),
            )
            .at_path(IDENT_TGT)
        })?;
        return Ok(Some(parse_tgt_text(text)));
    }
    Ok(None)
}

fn read_identification(
    source: &dyn ImportSource,
    relative_path: &str,
) -> Result<Vec<u8>, ImportError> {
    let bytes = source.read_file(relative_path, IDENTIFICATION_MAX_BYTES)?;
    if bytes.len() > IDENTIFICATION_MAX_BYTES {
        return Err(ImportError::new(
            ImportErrorKind::SizeLimitExceeded,
            format!(
                "identification file exceeds the {}-byte limit; found {} bytes",
                IDENTIFICATION_MAX_BYTES,
                bytes.len()
            ),
        )
        .at_path(relative_path));
    }
    Ok(bytes)
}

fn parse_json_bytes(bytes: &[u8]) -> Result<MachineInfo, JsonIdentificationError> {
    // Qt's JSON reader accepts the UTF-8 BOM emitted by some ResMed media.
    // `serde_json` does not, so remove only that exact optional prefix before
    // parsing to preserve the pinned loader's accepted input surface.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let document: Value = serde_json::from_slice(bytes).map_err(JsonIdentificationError::Json)?;
    let product = document
        .get("FlowGenerator")
        .and_then(|value| value.get("IdentificationProfiles"))
        .and_then(|value| value.get("Product"))
        .and_then(Value::as_object)
        .ok_or(JsonIdentificationError::MissingField(
            "FlowGenerator.IdentificationProfiles.Product",
        ))?;

    // OSCAR's `scanProductObject` copies these three strings directly. Preserve
    // them verbatim in the corresponding DTO fields. OSCAR derives `series`
    // with `model.left(model.indexOf("11") + 2)`, which truncates product names
    // without `11` and omits the display space in names such as `AirSense11`.
    // OPAP intentionally corrects only that derived family value. If no product
    // name exists, OPAP keeps the family empty rather than inheriting OSCAR's
    // `newInfo()` default of `S9`.
    let mut info = MachineInfo::default();
    info.serial = json_string(product.get("SerialNumber"));
    info.model_number = json_string(product.get("ProductCode"));
    info.source_model = json_string(product.get("ProductName"));
    info.model = info.source_model.clone();
    info.series = series_for_product_name(&info.model)
        .unwrap_or_default()
        .to_owned();
    Ok(info)
}

fn json_string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn parse_tgt_text(text: &str) -> MachineInfo {
    let mut info = MachineInfo::default();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        let Some((raw_key, raw_value)) = line.split_once(' ') else {
            continue;
        };
        let Some(key) = raw_key.strip_prefix('#') else {
            continue;
        };
        let value = raw_value.trim();

        match key {
            "SRN" => info.serial = value.to_owned(),
            "PCD" => info.model_number = value.to_owned(),
            "PNA" => {
                info.source_model = value.to_owned();
                let (model, series) = normalize_tgt_model(value);
                info.model = model;
                info.series = series.to_owned();
            }
            _ => {}
        }
    }

    info
}

fn normalize_tgt_model(value: &str) -> (String, &'static str) {
    // OSCAR replaces underscores and recognizes a small, case-sensitive set of
    // families before assuming S9. OPAP intentionally makes family recognition
    // case-insensitive, includes newer known families, and normalizes incidental
    // whitespace. Serial and product-code values receive only the TGT line
    // parser's outer-whitespace trimming; no family normalization touches them.
    let mut model = value.replace(['_', '('], " ").replace(')', "");
    model = model.split_whitespace().collect::<Vec<_>>().join(" ");
    let series = series_for_product_name(&model).unwrap_or("S9");
    if series == "S9" && !model.to_ascii_lowercase().starts_with("s9") {
        model = format!("S9 {model}");
    }

    (model, series)
}

fn series_for_product_name(model: &str) -> Option<&'static str> {
    let compact: String = model
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if compact.contains("airsense11") {
        Some("AirSense 11")
    } else if compact.contains("aircurve11") {
        Some("AirCurve 11")
    } else if compact.contains("airsense10") {
        Some("AirSense 10")
    } else if compact.contains("sleepmate10") {
        Some("Sleepmate 10")
    } else if compact.contains("aircurve10") {
        Some("AirCurve 10")
    } else if compact.contains("lumis") {
        Some("Lumis")
    } else if compact.starts_with("s9") {
        Some("S9")
    } else {
        None
    }
}

#[cfg(all(test, feature = "native-fs", not(target_family = "wasm")))]
mod tests {
    use super::*;
    use crate::importer::{ImportSource, SourceEntry};
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::TempDir;

    struct MemorySource {
        inventory: SourceInventory,
        files: BTreeMap<String, Vec<u8>>,
    }

    struct LimitIgnoringSource(MemorySource);

    impl ImportSource for MemorySource {
        fn inventory(&self) -> Result<SourceInventory, ImportError> {
            Ok(self.inventory.clone())
        }

        fn read_file(&self, relative_path: &str, max_bytes: usize) -> Result<Vec<u8>, ImportError> {
            let bytes = self.files.get(relative_path).ok_or_else(|| {
                ImportError::new(ImportErrorKind::Source, "missing memory file")
                    .at_path(relative_path)
            })?;
            if bytes.len() > max_bytes {
                return Err(ImportError::new(
                    ImportErrorKind::SizeLimitExceeded,
                    "memory file exceeds requested limit",
                )
                .at_path(relative_path));
            }
            Ok(bytes.clone())
        }
    }

    impl ImportSource for LimitIgnoringSource {
        fn inventory(&self) -> Result<SourceInventory, ImportError> {
            self.0.inventory()
        }

        fn read_file(
            &self,
            relative_path: &str,
            _max_bytes: usize,
        ) -> Result<Vec<u8>, ImportError> {
            self.0.files.get(relative_path).cloned().ok_or_else(|| {
                ImportError::new(ImportErrorKind::Source, "missing memory file")
                    .at_path(relative_path)
            })
        }
    }

    fn memory_card(identification: Option<(&str, &[u8])>) -> MemorySource {
        let mut entries = vec![
            SourceEntry {
                relative_path: "DATALOG/20260101/20260101_220000_BRP.edf".to_owned(),
                kind: SourceEntryKind::File,
                size_bytes: 3,
            },
            SourceEntry {
                relative_path: STR_EDF.to_owned(),
                kind: SourceEntryKind::File,
                size_bytes: 0,
            },
        ];
        let mut files = BTreeMap::new();
        if let Some((path, bytes)) = identification {
            entries.push(SourceEntry {
                relative_path: path.to_owned(),
                kind: SourceEntryKind::File,
                size_bytes: u64::try_from(bytes.len()).expect("fixture length"),
            });
            files.insert(path.to_owned(), bytes.to_vec());
        }
        MemorySource {
            inventory: SourceInventory {
                entries,
                total_file_bytes: 3,
            },
            files,
        }
    }

    fn card() -> TempDir {
        let directory = TempDir::new().expect("temporary directory");
        fs::create_dir(directory.path().join(DATALOG)).expect("DATALOG directory");
        fs::write(directory.path().join(STR_EDF), []).expect("STR.edf");
        directory
    }

    #[test]
    fn bounded_detection_applies_pinned_oscar_code_required_entries() {
        let directory = TempDir::new().expect("temporary directory");
        assert!(!detect_card(directory.path()));
        fs::create_dir(directory.path().join(DATALOG)).expect("DATALOG directory");
        assert!(!detect_card(directory.path()));
        fs::write(directory.path().join("STR.edf.gz"), []).expect("compressed STR");
        assert!(
            !detect_card(directory.path()),
            "OSCAR Detect requires literal uncompressed STR.edf even though Open can accept gzip"
        );
        fs::write(directory.path().join(STR_EDF), []).expect("STR.edf");
        assert!(detect_card(directory.path()));
        assert!(
            !detect_card(directory.path().join(DATALOG)),
            "OSCAR Detect does not perform Open's DATALOG-to-root normalization"
        );
    }

    #[test]
    fn detection_does_not_traverse_unrelated_deep_card_contents() {
        let directory = card();
        fs::write(
            directory.path().join(IDENT_TGT),
            b"#SRN shallow-identity\n#PNA AirSense_10\n#PCD 37000\n",
        )
        .expect("identification");
        let mut unrelated = directory.path().join("unrelated");
        for index in 0..=crate::HARD_MAX_INVENTORY_DEPTH {
            unrelated.push(format!("level-{index}"));
        }
        fs::create_dir_all(unrelated).expect("deep unrelated tree");

        assert!(detect_card(directory.path()));
        assert_eq!(
            read_machine_info(directory.path())
                .expect("shallow identity read")
                .serial,
            "shallow-identity"
        );
        assert!(
            DirectorySource::open(directory.path())
                .expect("directory source")
                .inventory()
                .is_err(),
            "the full importer inventory still enforces its recursion budget"
        );
    }

    #[test]
    fn detection_is_not_oscar_open_import_readiness() {
        let directory = card();
        fs::write(directory.path().join(STR_EDF), b"not an EDF").expect("corrupt STR fixture");

        assert!(detect_card(directory.path()));
        let discovery = ResmedImporter
            .discover(&DirectorySource::open(directory.path()).expect("directory source"))
            .expect("signature discovery")
            .expect("ResMed signature");
        assert!(discovery.device.machine.serial.is_empty());
        assert_eq!(discovery.warnings[0].code, "missing_identification");
    }

    #[test]
    fn discovery_does_not_attempt_oscars_verified_str_summary_path() {
        let directory = card();
        fs::write(directory.path().join(STR_EDF), b"not an EDF").expect("corrupt STR fixture");
        fs::write(
            directory.path().join(IDENT_TGT),
            b"#SRN identity-only-serial\n#PNA AirSense_10\n#PCD 37000\n",
        )
        .expect("identification");

        let discovery = ResmedImporter
            .discover(&DirectorySource::open(directory.path()).expect("directory source"))
            .expect("signature discovery")
            .expect("ResMed signature");
        assert_eq!(discovery.device.machine.serial, "identity-only-serial");
        assert!(discovery.warnings.is_empty());
    }

    #[test]
    fn discovery_does_not_yet_reject_an_empty_identification_serial() {
        let source = memory_card(Some((IDENT_TGT, b"#PNA AirSense_10_AutoSet\n#PCD 37028\n")));

        let discovery = ResmedImporter
            .discover(&source)
            .expect("signature discovery")
            .expect("ResMed signature");
        assert!(discovery.device.machine.serial.is_empty());
        assert!(discovery.warnings.is_empty());
    }

    #[test]
    fn parses_legacy_tgt_identification() {
        let directory = card();
        fs::write(
            directory.path().join(IDENT_TGT),
            "#SRN 23123456789\n#PNA AirSense_10_AutoSet\n#PCD 37028\n",
        )
        .expect("Identification.tgt");

        assert_eq!(
            read_machine_info(directory.path()).expect("machine info"),
            MachineInfo {
                brand: "ResMed".to_owned(),
                model: "AirSense 10 AutoSet".to_owned(),
                source_model: "AirSense_10_AutoSet".to_owned(),
                model_number: "37028".to_owned(),
                serial: "23123456789".to_owned(),
                series: "AirSense 10".to_owned(),
            }
        );
    }

    #[test]
    fn opap_correction_normalizes_legacy_s9_name() {
        let directory = card();
        fs::write(
            directory.path().join(IDENT_TGT),
            "#PNA (VPAP_Adapt)\n#SRN 123\n#PCD 36037\n",
        )
        .expect("Identification.tgt");

        let info = read_machine_info(directory.path()).expect("machine info");
        assert_eq!(info.series, "S9");
        assert_eq!(info.model, "S9 VPAP Adapt");
        assert_eq!(info.source_model, "(VPAP_Adapt)");
    }

    #[test]
    fn opap_correction_collapses_legacy_tgt_model_whitespace() {
        let (plain, plain_series) = normalize_tgt_model("VPAP_Adapt");
        let (parenthesized, parenthesized_series) = normalize_tgt_model("(VPAP_Adapt)");

        assert_eq!(plain_series, "S9");
        assert_eq!(parenthesized_series, "S9");
        assert_eq!(plain, "S9 VPAP Adapt");
        assert_eq!(parenthesized, "S9 VPAP Adapt");
    }

    #[test]
    fn json_file_precedence_matches_pinned_oscar_code() {
        let directory = card();
        fs::write(
            directory.path().join(IDENT_TGT),
            "#SRN old\n#PNA AirSense_10\n#PCD old\n",
        )
        .expect("Identification.tgt");
        fs::write(
            directory.path().join(IDENT_JSON),
            r#"{
              "FlowGenerator": {
                "IdentificationProfiles": {
                  "Product": {
                    "SerialNumber": "new",
                    "ProductCode": "39001",
                    "ProductName": "AirSense11 AutoSet"
                  }
                }
              }
            }"#,
        )
        .expect("Identification.json");

        let info = read_machine_info(directory.path()).expect("machine info");
        assert_eq!(info.serial, "new");
        assert_eq!(info.model_number, "39001");
        assert_eq!(info.model, "AirSense11 AutoSet");
        assert_eq!(info.source_model, "AirSense11 AutoSet");
    }

    #[test]
    fn opap_correction_normalizes_json_family_without_mutating_product_name() {
        let source = memory_card(Some((
            IDENT_JSON,
            br#"{
              "FlowGenerator": {
                "IdentificationProfiles": {
                  "Product": {
                    "SerialNumber": "new",
                    "ProductCode": "39001",
                    "ProductName": "AirSense11 AutoSet"
                  }
                }
              }
            }"#,
        )));

        let machine = ResmedImporter
            .discover(&source)
            .expect("discovery")
            .expect("ResMed signature")
            .device
            .machine;
        assert_eq!(machine.model, "AirSense11 AutoSet");
        assert_eq!(machine.source_model, "AirSense11 AutoSet");
        assert_eq!(machine.series, "AirSense 11");
    }

    #[test]
    fn json_raw_identity_fields_match_pinned_scan_product_object() {
        let source = memory_card(Some((
            IDENT_JSON,
            br#"{
              "FlowGenerator": {
                "IdentificationProfiles": {
                  "Product": {
                    "SerialNumber": "  Raw Serial  ",
                    "ProductCode": " 039001-X ",
                    "ProductName": "Prototype family 11  "
                  }
                }
              }
            }"#,
        )));

        let discovery = ResmedImporter
            .discover(&source)
            .expect("discovery")
            .expect("ResMed signature");
        let machine = discovery.device.machine;
        assert_eq!(machine.serial, "  Raw Serial  ");
        assert_eq!(machine.model_number, " 039001-X ");
        assert_eq!(machine.model, "Prototype family 11  ");
        assert_eq!(machine.source_model, "Prototype family 11  ");
    }

    #[test]
    fn json_utf8_bom_is_accepted_like_qjsondocument() {
        let source = memory_card(Some((
            IDENT_JSON,
            b"\xEF\xBB\xBF{\"FlowGenerator\":{\"IdentificationProfiles\":{\"Product\":{\"SerialNumber\":\"bom-serial\",\"ProductCode\":\"39001\",\"ProductName\":\"AirSense11 AutoSet\"}}}}",
        )));

        let machine = ResmedImporter
            .discover(&source)
            .expect("discovery")
            .expect("ResMed signature")
            .device
            .machine;
        assert_eq!(machine.serial, "bom-serial");
        assert_eq!(machine.model_number, "39001");
        assert_eq!(machine.source_model, "AirSense11 AutoSet");
        assert_eq!(machine.series, "AirSense 11");
    }

    #[test]
    fn opap_correction_recognizes_case_insensitive_tgt_family() {
        let source = memory_card(Some((
            IDENT_TGT,
            b"#SRN raw-serial\n#PNA airsense_10_autoset\n#PCD raw-code\n",
        )));

        let discovery = ResmedImporter
            .discover(&source)
            .expect("discovery")
            .expect("ResMed signature");
        let machine = discovery.device.machine;
        assert_eq!(machine.serial, "raw-serial");
        assert_eq!(machine.model_number, "raw-code");
        assert_eq!(machine.model, "airsense 10 autoset");
        assert_eq!(machine.source_model, "airsense_10_autoset");
        assert_eq!(machine.series, "AirSense 10");
    }

    #[test]
    fn opap_correction_does_not_invent_s9_when_product_name_is_absent() {
        let tgt = ResmedImporter
            .discover(&memory_card(Some((
                IDENT_TGT,
                b"#SRN serial-without-name\n#PCD code-without-name\n",
            ))))
            .expect("TGT discovery")
            .expect("ResMed signature")
            .device
            .machine;
        assert_eq!(tgt.serial, "serial-without-name");
        assert_eq!(tgt.model_number, "code-without-name");
        assert!(tgt.model.is_empty());
        assert!(tgt.source_model.is_empty());
        assert!(tgt.series.is_empty());

        let json = ResmedImporter
            .discover(&memory_card(Some((
                IDENT_JSON,
                br#"{
                  "FlowGenerator": {
                    "IdentificationProfiles": {
                      "Product": {
                        "SerialNumber": "json-serial",
                        "ProductCode": "json-code"
                      }
                    }
                  }
                }"#,
            ))))
            .expect("JSON discovery")
            .expect("ResMed signature")
            .device
            .machine;
        assert_eq!(json.serial, "json-serial");
        assert_eq!(json.model_number, "json-code");
        assert!(json.model.is_empty());
        assert!(json.source_model.is_empty());
        assert!(json.series.is_empty());
    }

    #[test]
    fn accepts_well_formed_tgt_keys_and_rejects_hardened_variants() {
        let source = memory_card(Some((
            IDENT_TGT,
            b"SRN unprefixed\n#SRN\ttab-separated\n#SRN accepted\n#SRN\njunk#SRN malformed\nPCD ignored\n#PCD 37028\n",
        )));

        let machine = ResmedImporter
            .discover(&source)
            .expect("discovery")
            .expect("ResMed signature")
            .device
            .machine;
        assert_eq!(machine.serial, "accepted");
        assert_eq!(machine.model_number, "37028");
    }

    #[test]
    fn inventories_card_files_recursively_and_classifies_roles() {
        let directory = card();
        let datalog = directory.path().join("DATALOG/20260101");
        fs::create_dir_all(&datalog).expect("dated DATALOG directory");
        fs::write(directory.path().join(IDENT_TGT), b"#SRN 123\n").expect("identity");
        fs::write(datalog.join("20260101_220000_BRP.edf"), b"1234").expect("BRP");
        fs::write(datalog.join("20260101_220000_EVE.edf.gz"), b"12").expect("EVE");
        fs::write(datalog.join("20260101_220000_PLD.edf"), b"1").expect("PLD");
        fs::write(datalog.join("20260101_220000_PLD.crc"), b"abc").expect("CRC");
        fs::write(directory.path().join("notes.txt"), b"note").expect("other file");

        let inventory = inventory_card(directory.path()).expect("card inventory");
        let roles: BTreeMap<_, _> = inventory
            .files
            .iter()
            .map(|file| (file.relative_path.as_str(), file.role))
            .collect();

        assert_eq!(
            roles["DATALOG/20260101/20260101_220000_BRP.edf"],
            CardFileRole::Waveform
        );
        assert_eq!(
            roles["DATALOG/20260101/20260101_220000_EVE.edf.gz"],
            CardFileRole::Events
        );
        assert_eq!(
            roles["DATALOG/20260101/20260101_220000_PLD.edf"],
            CardFileRole::Detail
        );
        assert_eq!(
            roles["DATALOG/20260101/20260101_220000_PLD.crc"],
            CardFileRole::Checksum
        );
        assert_eq!(roles[IDENT_TGT], CardFileRole::Identification);
        assert_eq!(roles[STR_EDF], CardFileRole::Summary);
        assert_eq!(roles["notes.txt"], CardFileRole::Other);
        assert_eq!(
            inventory.total_bytes,
            inventory
                .files
                .iter()
                .map(|file| file.size_bytes)
                .sum::<u64>()
        );
        assert!(
            inventory
                .files
                .windows(2)
                .all(|pair| pair[0].relative_path < pair[1].relative_path)
        );
    }

    #[test]
    fn portable_discovery_accepts_browser_file_only_inventory() {
        let source = memory_card(Some((
            IDENT_TGT,
            b"#SRN 123\n#PNA AirSense_10_AutoSet\n#PCD 37028\n",
        )));

        let discovery = ResmedImporter
            .discover(&source)
            .expect("discovery")
            .expect("ResMed card");

        assert_eq!(discovery.device.importer_id, IMPORTER_ID);
        assert_eq!(discovery.device.machine.serial, "123");
        assert_eq!(discovery.device.machine.series, "AirSense 10");
        assert!(discovery.warnings.is_empty());
    }

    #[test]
    fn portable_discovery_warns_when_identity_is_missing() {
        let discovery = ResmedImporter
            .discover(&memory_card(None))
            .expect("discovery")
            .expect("ResMed card");

        assert_eq!(discovery.device.machine.brand, "ResMed");
        assert_eq!(discovery.warnings.len(), 1);
        assert_eq!(discovery.warnings[0].code, "missing_identification");
    }

    #[test]
    fn malformed_preferred_json_does_not_fall_back_to_tgt() {
        let mut source = memory_card(Some((IDENT_JSON, b"not json")));
        source.inventory.entries.push(SourceEntry {
            relative_path: IDENT_TGT.to_owned(),
            kind: SourceEntryKind::File,
            size_bytes: 9,
        });
        source
            .files
            .insert(IDENT_TGT.to_owned(), b"#SRN old\n".to_vec());

        let error = ResmedImporter
            .discover(&source)
            .expect_err("malformed JSON must fail");

        assert_eq!(error.kind, ImportErrorKind::InvalidData);
        assert_eq!(error.relative_path.as_deref(), Some(IDENT_JSON));
    }

    #[test]
    fn rejects_oversized_identification_even_if_source_ignores_limit() {
        let oversized = vec![b' '; IDENTIFICATION_MAX_BYTES + 1];
        let source = LimitIgnoringSource(memory_card(Some((IDENT_JSON, &oversized))));

        let error = ResmedImporter
            .discover(&source)
            .expect_err("oversized identification must fail");

        assert_eq!(error.kind, ImportErrorKind::SizeLimitExceeded);
        assert_eq!(error.relative_path.as_deref(), Some(IDENT_JSON));
    }

    #[test]
    fn session_import_requires_explicit_clock_context() {
        let source = memory_card(Some((IDENT_TGT, b"#SRN 123\n")));

        let error = ResmedImporter
            .import(&source, &ImportOptions::default())
            .expect_err("clock context is required");

        assert_eq!(error.kind, ImportErrorKind::InvalidConfiguration);
    }
}
