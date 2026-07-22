//! ResMed card detection and identification parsing.
//!
//! Behavioral reference: OSCAR-SQL `resmed_loader.cpp` at the revision pinned
//! in `compat/oscar-sql-revision.txt`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DATALOG: &str = "DATALOG";
const STR_EDF: &str = "STR.edf";
const IDENT_TGT: &str = "Identification.tgt";
const IDENT_JSON: &str = "Identification.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineInfo {
    pub brand: String,
    pub model: String,
    pub model_number: String,
    pub serial: String,
    pub series: String,
}

impl Default for MachineInfo {
    fn default() -> Self {
        Self {
            brand: "ResMed".to_owned(),
            model: String::new(),
            model_number: String::new(),
            serial: String::new(),
            series: String::new(),
        }
    }
}

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
}

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
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Matches OSCAR's ResMed detection rule: a directory containing both
/// `DATALOG/` and `STR.edf` is a ResMed card.
pub fn detect_card(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    path.is_dir() && path.join(DATALOG).is_dir() && path.join(STR_EDF).is_file()
}

/// Reads machine metadata, preferring AirSense 11 `Identification.json` when
/// both JSON and the legacy TGT file are present, as OSCAR does.
pub fn read_machine_info(path: impl AsRef<Path>) -> Result<MachineInfo, Error> {
    let path = path.as_ref();
    if !detect_card(path) {
        return Err(Error::NotResmedCard(path.to_owned()));
    }

    let json_path = path.join(IDENT_JSON);
    if json_path.is_file() {
        return parse_json_file(&json_path);
    }

    let tgt_path = path.join(IDENT_TGT);
    if tgt_path.is_file() {
        return parse_tgt_file(&tgt_path);
    }

    Err(Error::MissingIdentification(path.to_owned()))
}

fn parse_json_file(path: &Path) -> Result<MachineInfo, Error> {
    let bytes = fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    let document: Value = serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: path.to_owned(),
        source,
    })?;
    let product = document
        .get("FlowGenerator")
        .and_then(|value| value.get("IdentificationProfiles"))
        .and_then(|value| value.get("Product"))
        .and_then(Value::as_object)
        .ok_or(Error::InvalidIdentificationJson(
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

fn parse_tgt_file(path: &Path) -> Result<MachineInfo, Error> {
    let text = fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
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

    Ok(info)
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
            model = format!("S9  {}", model.trim_start());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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
}
