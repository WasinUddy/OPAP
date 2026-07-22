// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2026 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from OSCAR-SQL:
// https://gitlab.com/CrimsonNape/OSCAR-SQL
// Upstream commit: 3741e5b423e4b5796c51a9d447e83b2525963d50
// Relevant upstream files:
// oscar/SleepLib/loader_plugins/resmed_loader.cpp
// oscar/SleepLib/loader_plugins/resmed_loader.h
// Modified: 2026-07-22

//! ResMed card detection, identification parsing, source inventory, and
//! bounded session-candidate indexing.
//!
//! Behavioral reference: OSCAR-SQL `resmed_loader.cpp` at the revision pinned
//! in `compat/oscar-sql-revision.txt`.

use crate::domain::{DeviceInfo, ImportWarning, WarningSeverity};
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
use crate::importer::DirectorySource;
use crate::importer::{
    DeviceDiscovery, ImportError, ImportErrorKind, ImportOptions, ImportSource, Importer,
    SourceEntryKind, SourceInventory,
};
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

mod session_index;

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
    /// `_EVE.edf`, `_CSL.edf`, or `_AEV.edf` annotations.
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
/// Discovery, inventory, and bounded EDF header indexing are implemented.
/// Clinical channel decoding is added in a later porting phase and currently returns
/// [`ImportErrorKind::UnsupportedOperation`].
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

/// Matches OSCAR's ResMed detection rule: a directory containing both
/// `DATALOG/` and `STR.edf` is a ResMed card.
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
pub fn detect_card(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    DirectorySource::open(path)
        .and_then(|source| source.inventory())
        .is_ok_and(|inventory| is_resmed_inventory(&inventory))
}

/// Reads machine metadata, preferring AirSense 11 `Identification.json` when
/// both JSON and the legacy TGT file are present, as OSCAR does.
#[cfg(all(feature = "native-fs", not(target_family = "wasm")))]
pub fn read_machine_info(path: impl AsRef<Path>) -> Result<MachineInfo, Error> {
    let path = path.as_ref();
    let source = open_native_source(path)?;
    let inventory = native_inventory(path, &source)?;
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
        _options: &ImportOptions,
    ) -> Result<crate::domain::ImportReport, ImportError> {
        if self.discover(source)?.is_none() {
            return Err(ImportError::new(
                ImportErrorKind::UnsupportedSource,
                "source is not a ResMed card",
            ));
        }

        Err(ImportError::new(
            ImportErrorKind::UnsupportedOperation,
            "ResMed clinical session decoding and import are not implemented yet",
        ))
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
    let document: Value = serde_json::from_slice(bytes).map_err(JsonIdentificationError::Json)?;
    let product = document
        .get("FlowGenerator")
        .and_then(|value| value.get("IdentificationProfiles"))
        .and_then(|value| value.get("Product"))
        .and_then(Value::as_object)
        .ok_or(JsonIdentificationError::MissingField(
            "FlowGenerator.IdentificationProfiles.Product",
        ))?;

    let mut info = MachineInfo::default();
    info.serial = json_string(product.get("SerialNumber"));
    info.model_number = json_string(product.get("ProductCode"));
    info.model = json_string(product.get("ProductName"));
    info.series = series_for_json_model(&info.model)
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
        let Some((raw_key, raw_value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let key = raw_key.strip_prefix('#').unwrap_or(raw_key);
        let value = raw_value.trim();

        match key {
            "SRN" => info.serial = value.to_owned(),
            "PCD" => info.model_number = value.to_owned(),
            "PNA" => {
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
    let mut model = value.replace('_', " ");
    let lower = model.to_ascii_lowercase();
    let series = if lower.contains("airsense 11") || lower.contains("airsense11") {
        "AirSense 11"
    } else if lower.contains("aircurve 11") || lower.contains("aircurve11") {
        "AirCurve 11"
    } else if lower.contains("airsense 10") {
        "AirSense 10"
    } else if lower.contains("sleepmate 10") {
        "Sleepmate 10"
    } else if lower.contains("aircurve 10") {
        "AirCurve 10"
    } else if lower.contains("lumis") {
        "Lumis"
    } else {
        model = model.replace('(', " ").replace(')', "");
        if !model.starts_with("S9") {
            model = model.replace("S9", "");
            model = format!("S9 {model}");
        }
        "S9"
    };

    (model.trim().to_owned(), series)
}

fn series_for_json_model(model: &str) -> Option<&'static str> {
    let lower = model.to_ascii_lowercase();
    if lower.contains("airsense11") || lower.contains("airsense 11") {
        Some("AirSense 11")
    } else if lower.contains("aircurve11") || lower.contains("aircurve 11") {
        Some("AirCurve 11")
    } else if lower.contains("airsense10") || lower.contains("airsense 10") {
        Some("AirSense 10")
    } else if lower.contains("aircurve10") || lower.contains("aircurve 10") {
        Some("AirCurve 10")
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
    fn detection_matches_oscar_required_entries() {
        let directory = TempDir::new().expect("temporary directory");
        assert!(!detect_card(directory.path()));
        fs::create_dir(directory.path().join(DATALOG)).expect("DATALOG directory");
        assert!(!detect_card(directory.path()));
        fs::write(directory.path().join(STR_EDF), []).expect("STR.edf");
        assert!(detect_card(directory.path()));
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
                model_number: "37028".to_owned(),
                serial: "23123456789".to_owned(),
                series: "AirSense 10".to_owned(),
            }
        );
    }

    #[test]
    fn normalizes_series_nine_names_like_oscar() {
        let directory = card();
        fs::write(
            directory.path().join(IDENT_TGT),
            "#PNA (VPAP_Adapt)\n#SRN 123\n#PCD 36037\n",
        )
        .expect("Identification.tgt");

        let info = read_machine_info(directory.path()).expect("machine info");
        assert_eq!(info.series, "S9");
        assert_eq!(info.model, "S9  VPAP Adapt");
    }

    #[test]
    fn preserves_oscar_series_nine_whitespace_quirk() {
        let (plain, plain_series) = normalize_tgt_model("VPAP_Adapt");
        let (parenthesized, parenthesized_series) = normalize_tgt_model("(VPAP_Adapt)");

        assert_eq!(plain_series, "S9");
        assert_eq!(parenthesized_series, "S9");
        assert_eq!(plain, "S9 VPAP Adapt");
        assert_eq!(parenthesized, "S9  VPAP Adapt");
    }

    #[test]
    fn json_identification_takes_precedence_over_tgt() {
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
        assert_eq!(info.series, "AirSense 11");
        assert_eq!(info.model_number, "39001");
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
    fn session_import_reports_intentionally_unimplemented_parser() {
        let source = memory_card(Some((IDENT_TGT, b"#SRN 123\n")));

        let error = ResmedImporter
            .import(&source, &ImportOptions::default())
            .expect_err("EDF parser is intentionally pending");

        assert_eq!(error.kind, ImportErrorKind::UnsupportedOperation);
    }
}
