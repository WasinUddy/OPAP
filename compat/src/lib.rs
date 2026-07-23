//! Strict, test-only differential comparison for OSCAR and OPAP exports.
//!
//! This crate is deliberately outside the production workspace. It compares
//! canonical summaries produced from the same private or synthetic card and
//! does not parse card data itself.

use opap_channels::{ChannelKind, ResmedFileKind, by_stable_key};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

pub const MANIFEST_SCHEMA_VERSION: &str = "opap-oscar-compat/v1";
pub const ORACLE_NAME: &str = "OSCAR";
pub const ORACLE_REPOSITORY: &str = "https://gitlab.com/CrimsonNape/OSCAR-code";
pub const ORACLE_REVISION: &str = "64c5e90a26f91fb15868bcfcccde0c1e1522ac86";
pub const ORACLE_EXPORT_SCHEMA_VERSION: &str = "opap-oscar-oracle/v1";
pub const SUBJECT_NAME: &str = "OPAP";
pub const WAVEFORM_DIGEST_ENCODING: &str = "edf-i16le-f32be-segments-v1";
pub const WAVEFORM_PREVIEW_SAMPLES: usize = 16;
pub const JSON_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Absolute tolerances used only for fields that are inherently floating point.
/// All other fields, including digests, timestamps, counts, and identifiers,
/// compare exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatTolerances {
    pub summary_metric_abs: f64,
    pub setting_number_abs: f64,
    pub event_value_abs: f64,
    pub waveform_sample_rate_hz_abs: f64,
    pub waveform_preview_sample_abs: f64,
}

pub const DEFAULT_FLOAT_TOLERANCES: FloatTolerances = FloatTolerances {
    summary_metric_abs: 1.0e-6,
    setting_number_abs: 1.0e-6,
    event_value_abs: 1.0e-6,
    waveform_sample_rate_hz_abs: 1.0e-9,
    waveform_preview_sample_abs: 1.0e-4,
};

impl FloatTolerances {
    pub fn named(self) -> [(&'static str, f64); 5] {
        [
            ("summary_metric_abs", self.summary_metric_abs),
            ("setting_number_abs", self.setting_number_abs),
            ("event_value_abs", self.event_value_abs),
            (
                "waveform_sample_rate_hz_abs",
                self.waveform_sample_rate_hz_abs,
            ),
            (
                "waveform_preview_sample_abs",
                self.waveform_preview_sample_abs,
            ),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityManifest {
    pub schema_version: String,
    pub oracle: OracleIdentity,
    pub producer: ProducerIdentity,
    pub fixture: FixtureIdentity,
    pub machine: Machine,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerRole {
    Oracle,
    Subject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerIdentity {
    pub role: ProducerRole,
    pub name: String,
    pub source_revision: String,
    pub adapter_attestation: AdapterAttestationKind,
    pub adapter_repository: String,
    pub adapter_revision: String,
    pub adapter_tree_sha256: String,
    pub adapter_conformance_sha256: String,
    pub adapter_clean: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterAttestationKind {
    SyntheticFixtureOnly,
    VerifiedCleanTree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleIdentity {
    pub name: String,
    pub repository: String,
    pub revision: String,
    pub export_schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureIdentity {
    pub case_id: String,
    pub source_sha256: String,
    pub synthetic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    pub manufacturer: String,
    pub model: String,
    pub model_number: String,
    pub serial_number: String,
    pub firmware: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub session_id: String,
    pub source_id_sha256: String,
    pub source_sha256: String,
    pub sha256: String,
    pub time: SessionTime,
    pub slices: SliceCollection,
    pub summary: SessionSummary,
    pub settings: SettingsCollection,
    pub events: EventCollection,
    pub waveforms: WaveformCollection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTime {
    pub start_utc: String,
    pub end_utc: String,
    pub start_local: String,
    pub end_local: String,
    #[serde(deserialize_with = "deserialize_required_option")]
    pub start_utc_offset_seconds: Option<i32>,
    #[serde(deserialize_with = "deserialize_required_option")]
    pub end_utc_offset_seconds: Option<i32>,
    /// v1 deliberately trusts only the source's endpoint offsets and does not
    /// claim an IANA zone identity without a pinned timezone database.
    pub timezone_basis: String,
    pub start_clock_correction_milliseconds: i64,
    pub end_clock_correction_milliseconds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceCollection {
    pub count: u64,
    pub sha256: String,
    pub items: Vec<SessionSlice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceStatus {
    MaskOn,
    MaskOff,
    EquipmentOff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSlice {
    pub sequence: u64,
    pub source_id_sha256: String,
    pub status: SliceStatus,
    pub start_offset_milliseconds: i64,
    pub end_offset_milliseconds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSummary {
    pub usage_milliseconds: u64,
    pub metric_count: u64,
    pub source_sha256: String,
    pub metrics: Vec<SummaryMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryMetric {
    pub key: String,
    pub value: NullableNumber,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsCollection {
    pub count: u64,
    pub sha256: String,
    pub items: Vec<Setting>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub key: String,
    pub value: SettingValue,
    /// UCUM-style unit where possible; an empty string means unitless.
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SettingValue {
    Number(f64),
    Integer(i64),
    Boolean(bool),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventCollection {
    pub channel_count: u64,
    pub sha256: String,
    pub channels: Vec<EventChannel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventChannel {
    pub channel_id: String,
    pub source_file_kind: ResmedFileKind,
    pub unit: String,
    pub count: u64,
    pub sha256: String,
    pub items: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub sequence: u64,
    pub source_id_sha256: String,
    pub offset_milliseconds: u64,
    #[serde(deserialize_with = "deserialize_required_option")]
    pub duration_milliseconds: Option<u64>,
    pub value: NullableNumber,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NullableNumber(pub Option<f64>);

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveformCollection {
    pub channel_count: u64,
    pub sha256: String,
    pub channels: Vec<WaveformChannel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveformChannel {
    pub channel_id: String,
    pub source_file_kind: ResmedFileKind,
    pub unit: String,
    pub sample_rate_hz: f64,
    pub sample_count: u64,
    pub start_offset_milliseconds: i64,
    pub segments: Vec<WaveformSegment>,
    /// Digest of raw source bytes for provenance.
    pub source_sha256: String,
    /// Digest of the importer-emitted decoded digital sequence and exact scale
    /// metadata, using [`waveform_semantic_sha256`].
    pub sha256: String,
    pub encoding: WaveformEncoding,
    pub head_samples: Vec<f64>,
    pub tail_samples: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveformSegment {
    pub sequence: u64,
    pub start_sample: u64,
    pub sample_count: u64,
    pub start_offset_milliseconds: i64,
    pub source_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveformEncoding {
    pub kind: String,
    pub digital_min: i32,
    pub digital_max: i32,
    pub physical_min_decimal: String,
    pub physical_max_decimal: String,
    pub samples_per_record: u32,
    pub record_duration_decimal: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

pub enum HarnessError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    InvalidManifest {
        label: String,
        issues: Vec<ValidationIssue>,
    },
    InvalidTolerances {
        issues: Vec<ValidationIssue>,
    },
}

impl fmt::Debug for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("HarnessError")
            .field(&self.to_string())
            .finish()
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { source, .. } => write!(f, "cannot read manifest: {source}"),
            Self::Json { source, .. } => write!(
                f,
                "invalid JSON manifest at line {}, column {}",
                source.line(),
                source.column()
            ),
            Self::InvalidManifest { label, issues } => {
                write!(f, "invalid {label} manifest")?;
                for issue in issues {
                    write!(f, "\n  {issue}")?;
                }
                Ok(())
            }
            Self::InvalidTolerances { issues } => {
                write!(f, "invalid float tolerance profile")?;
                for issue in issues {
                    write!(f, "\n  {issue}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for HarnessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Underlying IO/JSON errors can echo private paths or malformed values.
        // Keep them programmatically stored, but do not expose them through the
        // standard error-chain formatter used by generic reporters.
        None
    }
}

pub fn load_manifest(path: impl AsRef<Path>) -> Result<CompatibilityManifest, HarnessError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| HarnessError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut value: serde_json::Value =
        serde_json::from_reader(BufReader::new(file)).map_err(|source| HarnessError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    normalize_json_integer_tokens(&mut value);
    serde_json::from_value(value).map_err(|source| HarnessError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn normalize_json_integer_tokens(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                normalize_json_integer_tokens(value);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                normalize_json_integer_tokens(value);
            }
        }
        serde_json::Value::Number(number) => {
            // `arbitrary_precision` preserves the original decimal/exponent
            // token. Normalize only values that are *exactly* mathematical
            // integers; converting through f64 would silently round values
            // near 2^53 and could turn malformed manifests into valid ones.
            if let Some(integer) = exact_json_safe_integer(number) {
                *number = serde_json::Number::from(integer);
            }
        }
        _ => {}
    }
}

fn exact_json_safe_integer(number: &serde_json::Number) -> Option<i64> {
    let token = number.as_str();
    let (negative, unsigned) = match token.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token),
    };

    let (mantissa, exponent_token) = match unsigned.find(['e', 'E']) {
        Some(index) => (&unsigned[..index], Some(&unsigned[index + 1..])),
        None => (unsigned, None),
    };
    let (whole, fraction, has_decimal_point) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, fraction, true),
        None => (mantissa, "", false),
    };
    if whole.is_empty()
        || (whole.len() > 1 && whole.starts_with('0'))
        || (has_decimal_point && fraction.is_empty())
    {
        return None;
    }
    if let Some(exponent) = exponent_token {
        let digits = exponent
            .strip_prefix('+')
            .or_else(|| exponent.strip_prefix('-'))
            .unwrap_or(exponent);
        if digits.is_empty() || !ascii_digits(digits.as_bytes()) {
            return None;
        }
    }

    let coefficient_digits = || whole.bytes().chain(fraction.bytes());
    let mut saw_nonzero = false;
    let mut significant_len = 0_usize;
    let mut trailing_zeros = 0_usize;
    for digit in coefficient_digits() {
        if !digit.is_ascii_digit() {
            return None;
        }
        if digit != b'0' {
            saw_nonzero = true;
            trailing_zeros = 0;
        }
        if saw_nonzero {
            significant_len = significant_len.checked_add(1)?;
            if digit == b'0' {
                trailing_zeros = trailing_zeros.checked_add(1)?;
            }
        }
    }
    if !saw_nonzero {
        return Some(0);
    }

    let exponent = exponent_token.map_or(Some(0_i128), |value| value.parse().ok())?;
    let scale = exponent.checked_sub(i128::try_from(fraction.len()).ok()?)?;
    let max_digits = JSON_SAFE_INTEGER.ilog10() as usize + 1;
    let (kept_len, appended_zeros) = if scale >= 0 {
        let available_digits = max_digits.checked_sub(significant_len)?;
        if scale > i128::try_from(available_digits).ok()? {
            return None;
        }
        (significant_len, usize::try_from(scale).ok()?)
    } else {
        let discarded_zeros = scale.checked_neg()?;
        if discarded_zeros > i128::try_from(trailing_zeros).ok()? {
            return None;
        }
        let discarded_zeros = usize::try_from(discarded_zeros).ok()?;
        let kept_len = significant_len.checked_sub(discarded_zeros)?;
        if kept_len > max_digits {
            return None;
        }
        (kept_len, 0)
    };

    let mut magnitude = 0_u64;
    let mut started = false;
    let mut taken = 0_usize;
    for digit in coefficient_digits() {
        if !started && digit == b'0' {
            continue;
        }
        started = true;
        if taken == kept_len {
            break;
        }
        magnitude = magnitude
            .checked_mul(10)?
            .checked_add(u64::from(digit - b'0'))?;
        taken += 1;
    }
    if taken != kept_len {
        return None;
    }
    for _ in 0..appended_zeros {
        magnitude = magnitude.checked_mul(10)?;
    }
    if magnitude > JSON_SAFE_INTEGER {
        return None;
    }

    let value = i64::try_from(magnitude).ok()?;
    Some(if negative { -value } else { value })
}

pub fn load_and_validate(
    path: impl AsRef<Path>,
    label: impl Into<String>,
) -> Result<CompatibilityManifest, HarnessError> {
    let manifest = load_manifest(path)?;
    let issues = validate(&manifest);
    if issues.is_empty() {
        Ok(manifest)
    } else {
        Err(HarnessError::InvalidManifest {
            label: label.into(),
            issues,
        })
    }
}

pub fn validate(manifest: &CompatibilityManifest) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    exact_contract(
        &mut issues,
        "$.schema_version",
        &manifest.schema_version,
        MANIFEST_SCHEMA_VERSION,
    );
    exact_contract(
        &mut issues,
        "$.oracle.name",
        &manifest.oracle.name,
        ORACLE_NAME,
    );
    exact_contract(
        &mut issues,
        "$.oracle.repository",
        &manifest.oracle.repository,
        ORACLE_REPOSITORY,
    );
    exact_contract(
        &mut issues,
        "$.oracle.revision",
        &manifest.oracle.revision,
        ORACLE_REVISION,
    );
    exact_contract(
        &mut issues,
        "$.oracle.export_schema_version",
        &manifest.oracle.export_schema_version,
        ORACLE_EXPORT_SCHEMA_VERSION,
    );
    validate_producer(&mut issues, &manifest.producer, manifest.fixture.synthetic);

    canonical_identifier(&mut issues, "$.fixture.case_id", &manifest.fixture.case_id);
    sha256(
        &mut issues,
        "$.fixture.source_sha256",
        &manifest.fixture.source_sha256,
    );

    non_empty(
        &mut issues,
        "$.machine.manufacturer",
        &manifest.machine.manufacturer,
    );
    non_empty(&mut issues, "$.machine.model", &manifest.machine.model);
    non_empty(
        &mut issues,
        "$.machine.serial_number",
        &manifest.machine.serial_number,
    );
    sha256(&mut issues, "$.machine.sha256", &manifest.machine.sha256);

    if manifest.sessions.is_empty() {
        issue(
            &mut issues,
            "$.sessions",
            "at least one session is required; an empty export is never a pass",
        );
    }
    sorted_unique_by(&mut issues, "$.sessions", &manifest.sessions, |session| {
        &session.source_id_sha256
    });

    for (session_index, session) in manifest.sessions.iter().enumerate() {
        validate_session(&mut issues, session_index, session);
    }
    issues
}

fn validate_producer(
    issues: &mut Vec<ValidationIssue>,
    producer: &ProducerIdentity,
    synthetic_fixture: bool,
) {
    match producer.role {
        ProducerRole::Oracle => {
            exact_contract(issues, "$.producer.name", &producer.name, ORACLE_NAME);
            exact_contract(
                issues,
                "$.producer.source_revision",
                &producer.source_revision,
                ORACLE_REVISION,
            );
        }
        ProducerRole::Subject => {
            exact_contract(issues, "$.producer.name", &producer.name, SUBJECT_NAME);
            git_revision(
                issues,
                "$.producer.source_revision",
                &producer.source_revision,
            );
        }
    }
    git_revision(
        issues,
        "$.producer.adapter_revision",
        &producer.adapter_revision,
    );
    if !producer.adapter_repository.starts_with("https://") {
        issue(
            issues,
            "$.producer.adapter_repository",
            "must identify a public HTTPS adapter repository",
        );
    }
    sha256(
        issues,
        "$.producer.adapter_tree_sha256",
        &producer.adapter_tree_sha256,
    );
    sha256(
        issues,
        "$.producer.adapter_conformance_sha256",
        &producer.adapter_conformance_sha256,
    );
    if !producer.adapter_clean {
        issue(
            issues,
            "$.producer.adapter_clean",
            "must attest a clean adapter tree",
        );
    }
    if !synthetic_fixture
        && producer.adapter_attestation != AdapterAttestationKind::VerifiedCleanTree
    {
        issue(
            issues,
            "$.producer.adapter_attestation",
            "real-card comparisons require a verified_clean_tree adapter attestation",
        );
    }
}

fn validate_session(issues: &mut Vec<ValidationIssue>, index: usize, session: &Session) {
    let path = format!("$.sessions[{index}]");
    canonical_identifier(issues, &format!("{path}.session_id"), &session.session_id);
    sha256(
        issues,
        &format!("{path}.source_id_sha256"),
        &session.source_id_sha256,
    );
    sha256(
        issues,
        &format!("{path}.source_sha256"),
        &session.source_sha256,
    );
    sha256(issues, &format!("{path}.sha256"), &session.sha256);
    validate_time(issues, &format!("{path}.time"), &session.time);
    let session_duration_milliseconds = parse_iso_timestamp(&session.time.end_utc, true)
        .zip(parse_iso_timestamp(&session.time.start_utc, true))
        .and_then(|(end, start)| end.checked_sub(start));
    validate_slices(
        issues,
        &path,
        &session.slices,
        session_duration_milliseconds,
    );
    validate_summary(
        issues,
        &path,
        &session.summary,
        &session.slices,
        session_duration_milliseconds,
    );

    count_matches(
        issues,
        &format!("{path}.settings.count"),
        session.settings.count,
        session.settings.items.len(),
    );
    json_safe_unsigned(
        issues,
        &format!("{path}.settings.count"),
        session.settings.count,
    );
    sha256(
        issues,
        &format!("{path}.settings.sha256"),
        &session.settings.sha256,
    );
    sorted_unique_by(
        issues,
        &format!("{path}.settings.items"),
        &session.settings.items,
        |setting| &setting.key,
    );
    for (setting_index, setting) in session.settings.items.iter().enumerate() {
        let setting_path = format!("{path}.settings.items[{setting_index}]");
        canonical_identifier(issues, &format!("{setting_path}.key"), &setting.key);
        match setting.value {
            SettingValue::Number(value) => {
                finite(issues, &format!("{setting_path}.value"), value);
            }
            SettingValue::Integer(value) => {
                json_safe_signed(issues, &format!("{setting_path}.value"), value);
            }
            SettingValue::Boolean(_) | SettingValue::Text(_) => {}
        }
    }

    count_matches(
        issues,
        &format!("{path}.events.channel_count"),
        session.events.channel_count,
        session.events.channels.len(),
    );
    json_safe_unsigned(
        issues,
        &format!("{path}.events.channel_count"),
        session.events.channel_count,
    );
    sha256(
        issues,
        &format!("{path}.events.sha256"),
        &session.events.sha256,
    );
    sorted_unique_by(
        issues,
        &format!("{path}.events.channels"),
        &session.events.channels,
        |channel| &channel.channel_id,
    );
    for (channel_index, channel) in session.events.channels.iter().enumerate() {
        validate_event_channel(
            issues,
            &path,
            channel_index,
            channel,
            session_duration_milliseconds,
        );
    }

    count_matches(
        issues,
        &format!("{path}.waveforms.channel_count"),
        session.waveforms.channel_count,
        session.waveforms.channels.len(),
    );
    json_safe_unsigned(
        issues,
        &format!("{path}.waveforms.channel_count"),
        session.waveforms.channel_count,
    );
    sha256(
        issues,
        &format!("{path}.waveforms.sha256"),
        &session.waveforms.sha256,
    );
    sorted_unique_by(
        issues,
        &format!("{path}.waveforms.channels"),
        &session.waveforms.channels,
        |channel| &channel.channel_id,
    );
    for (channel_index, channel) in session.waveforms.channels.iter().enumerate() {
        validate_waveform_channel(
            issues,
            &path,
            channel_index,
            channel,
            session_duration_milliseconds,
        );
    }

    let computed_event_digest = event_collection_sha256(
        session
            .events
            .channels
            .iter()
            .map(|channel| (channel.channel_id.as_str(), channel.sha256.as_str())),
    );
    if session.events.sha256 != computed_event_digest {
        issue(
            issues,
            &format!("{path}.events.sha256"),
            "does not match the canonical event collection digest",
        );
    }
    let computed_waveform_digest =
        waveform_collection_sha256(session.waveforms.channels.iter().map(|channel| {
            (
                channel.channel_id.as_str(),
                channel.source_sha256.as_str(),
                channel.sha256.as_str(),
            )
        }));
    if session.waveforms.sha256 != computed_waveform_digest {
        issue(
            issues,
            &format!("{path}.waveforms.sha256"),
            "does not match the canonical waveform collection digest",
        );
    }
    let computed_session_digest = session_aggregate_sha256(
        &session.source_id_sha256,
        &session.source_sha256,
        &session.slices.sha256,
        &session.summary.source_sha256,
        &session.settings.sha256,
        &computed_event_digest,
        &computed_waveform_digest,
    );
    if session.sha256 != computed_session_digest {
        issue(
            issues,
            &format!("{path}.sha256"),
            "does not match the canonical session aggregate digest",
        );
    }
}

fn validate_slices(
    issues: &mut Vec<ValidationIssue>,
    session_path: &str,
    slices: &SliceCollection,
    session_duration_milliseconds: Option<i64>,
) {
    let path = format!("{session_path}.slices");
    count_matches(
        issues,
        &format!("{path}.count"),
        slices.count,
        slices.items.len(),
    );
    json_safe_unsigned(issues, &format!("{path}.count"), slices.count);
    sha256(issues, &format!("{path}.sha256"), &slices.sha256);
    if slices.items.is_empty() {
        issue(
            issues,
            &format!("{path}.items"),
            "at least one session slice is required",
        );
    }

    let mut previous_end = None;
    for (index, slice) in slices.items.iter().enumerate() {
        let item_path = format!("{path}.items[{index}]");
        if slice.sequence != index as u64 {
            issue(
                issues,
                &format!("{item_path}.sequence"),
                "slice sequence must be zero-based and contiguous",
            );
        }
        json_safe_unsigned(issues, &format!("{item_path}.sequence"), slice.sequence);
        sha256(
            issues,
            &format!("{item_path}.source_id_sha256"),
            &slice.source_id_sha256,
        );
        for (field, value) in [
            ("start_offset_milliseconds", slice.start_offset_milliseconds),
            ("end_offset_milliseconds", slice.end_offset_milliseconds),
        ] {
            json_safe_signed(issues, &format!("{item_path}.{field}"), value);
            if value < 0 {
                issue(
                    issues,
                    &format!("{item_path}.{field}"),
                    "must be nonnegative",
                );
            }
        }
        if slice.end_offset_milliseconds <= slice.start_offset_milliseconds {
            issue(
                issues,
                &format!("{item_path}.end_offset_milliseconds"),
                "must be later than start_offset_milliseconds",
            );
        }
        if previous_end.is_some_and(|end| slice.start_offset_milliseconds < end) {
            issue(issues, &item_path, "session slices must not overlap");
        }
        if session_duration_milliseconds
            .is_some_and(|duration| slice.end_offset_milliseconds > duration)
        {
            issue(
                issues,
                &item_path,
                "session slice extends beyond the session end",
            );
        }
        previous_end = Some(slice.end_offset_milliseconds);
    }

    if slices.sha256 != slice_collection_sha256(&slices.items) {
        issue(
            issues,
            &format!("{path}.sha256"),
            "does not match the canonical slice collection digest",
        );
    }
}

fn validate_summary(
    issues: &mut Vec<ValidationIssue>,
    session_path: &str,
    summary: &SessionSummary,
    slices: &SliceCollection,
    session_duration_milliseconds: Option<i64>,
) {
    let path = format!("{session_path}.summary");
    json_safe_unsigned(
        issues,
        &format!("{path}.usage_milliseconds"),
        summary.usage_milliseconds,
    );
    count_matches(
        issues,
        &format!("{path}.metric_count"),
        summary.metric_count,
        summary.metrics.len(),
    );
    json_safe_unsigned(
        issues,
        &format!("{path}.metric_count"),
        summary.metric_count,
    );
    sha256(
        issues,
        &format!("{path}.source_sha256"),
        &summary.source_sha256,
    );
    sorted_unique_by(
        issues,
        &format!("{path}.metrics"),
        &summary.metrics,
        |metric| &metric.key,
    );
    for (index, metric) in summary.metrics.iter().enumerate() {
        let metric_path = format!("{path}.metrics[{index}]");
        canonical_identifier(issues, &format!("{metric_path}.key"), &metric.key);
        if let Some(value) = metric.value.0 {
            finite(issues, &format!("{metric_path}.value"), value);
        }
    }

    let mask_on_milliseconds = slices
        .items
        .iter()
        .filter(|slice| slice.status == SliceStatus::MaskOn)
        .try_fold(0_u64, |total, slice| {
            let duration = slice
                .end_offset_milliseconds
                .checked_sub(slice.start_offset_milliseconds)?;
            total.checked_add(u64::try_from(duration).ok()?)
        });
    if mask_on_milliseconds != Some(summary.usage_milliseconds) {
        issue(
            issues,
            &format!("{path}.usage_milliseconds"),
            "must equal the exact duration of mask_on session slices",
        );
    }
    if session_duration_milliseconds.is_some_and(|duration| {
        u64::try_from(duration)
            .ok()
            .is_some_and(|duration| summary.usage_milliseconds > duration)
    }) {
        issue(
            issues,
            &format!("{path}.usage_milliseconds"),
            "must not exceed the session duration",
        );
    }
}

fn validate_time(issues: &mut Vec<ValidationIssue>, path: &str, time: &SessionTime) {
    let start_utc = utc_timestamp(issues, &format!("{path}.start_utc"), &time.start_utc);
    let end_utc = utc_timestamp(issues, &format!("{path}.end_utc"), &time.end_utc);
    let start_local = local_timestamp(issues, &format!("{path}.start_local"), &time.start_local);
    let end_local = local_timestamp(issues, &format!("{path}.end_local"), &time.end_local);
    for (field, offset) in [
        ("start_utc_offset_seconds", time.start_utc_offset_seconds),
        ("end_utc_offset_seconds", time.end_utc_offset_seconds),
    ] {
        if offset.is_some_and(|offset| !(-64_800..=64_800).contains(&offset)) {
            issue(
                issues,
                &format!("{path}.{field}"),
                "UTC offset must be between -64800 and 64800 seconds",
            );
        }
    }
    exact_contract(
        issues,
        &format!("{path}.timezone_basis"),
        &time.timezone_basis,
        "source_endpoint_metadata",
    );
    for (field, correction) in [
        (
            "start_clock_correction_milliseconds",
            time.start_clock_correction_milliseconds,
        ),
        (
            "end_clock_correction_milliseconds",
            time.end_clock_correction_milliseconds,
        ),
    ] {
        json_safe_signed(issues, &format!("{path}.{field}"), correction);
    }
    if let (Some(start), Some(end)) = (start_utc, end_utc)
        && start >= end
    {
        issue(
            issues,
            &format!("{path}.end_utc"),
            "must be later than start_utc",
        );
    }
    for (field, utc, local, offset, correction) in [
        (
            "start_utc",
            start_utc,
            start_local,
            time.start_utc_offset_seconds,
            time.start_clock_correction_milliseconds,
        ),
        (
            "end_utc",
            end_utc,
            end_local,
            time.end_utc_offset_seconds,
            time.end_clock_correction_milliseconds,
        ),
    ] {
        if let (Some(utc), Some(local), Some(offset)) = (utc, local, offset) {
            let normalized = local
                .checked_sub(i64::from(offset) * 1_000)
                .and_then(|value| value.checked_add(correction));
            if Some(utc) != normalized {
                issue(
                    issues,
                    &format!("{path}.{field}"),
                    "is inconsistent with local time, UTC offset, and clock correction",
                );
            }
        }
    }
}

fn validate_event_channel(
    issues: &mut Vec<ValidationIssue>,
    session_path: &str,
    index: usize,
    channel: &EventChannel,
    session_duration_milliseconds: Option<i64>,
) {
    let path = format!("{session_path}.events.channels[{index}]");
    canonical_identifier(issues, &format!("{path}.channel_id"), &channel.channel_id);
    validate_registered_channel(
        issues,
        &path,
        &channel.channel_id,
        &channel.unit,
        ChannelKind::Event,
        channel.source_file_kind,
    );
    sha256(issues, &format!("{path}.sha256"), &channel.sha256);
    count_matches(
        issues,
        &format!("{path}.count"),
        channel.count,
        channel.items.len(),
    );
    json_safe_unsigned(issues, &format!("{path}.count"), channel.count);
    for (event_index, event) in channel.items.iter().enumerate() {
        let event_path = format!("{path}.items[{event_index}]");
        if event.sequence != event_index as u64 {
            issue(
                issues,
                &format!("{event_path}.sequence"),
                "event sequence must be zero-based and contiguous",
            );
        }
        json_safe_unsigned(issues, &format!("{event_path}.sequence"), event.sequence);
        sha256(
            issues,
            &format!("{event_path}.source_id_sha256"),
            &event.source_id_sha256,
        );
        json_safe_unsigned(
            issues,
            &format!("{event_path}.offset_milliseconds"),
            event.offset_milliseconds,
        );
        if let Some(duration) = event.duration_milliseconds {
            json_safe_unsigned(
                issues,
                &format!("{event_path}.duration_milliseconds"),
                duration,
            );
        }
        if let Some(value) = event.value.0 {
            finite(issues, &format!("{event_path}.value"), value);
        }
        if let Some(session_duration) = session_duration_milliseconds {
            let event_end = event
                .offset_milliseconds
                .checked_add(event.duration_milliseconds.unwrap_or(0));
            if event_end.is_none_or(|end| {
                u64::try_from(session_duration)
                    .ok()
                    .is_none_or(|duration| end > duration)
            }) {
                issue(issues, &event_path, "event extends beyond the session end");
            }
        }
    }
}

fn validate_waveform_channel(
    issues: &mut Vec<ValidationIssue>,
    session_path: &str,
    index: usize,
    channel: &WaveformChannel,
    session_duration_milliseconds: Option<i64>,
) {
    let path = format!("{session_path}.waveforms.channels[{index}]");
    canonical_identifier(issues, &format!("{path}.channel_id"), &channel.channel_id);
    validate_registered_channel(
        issues,
        &path,
        &channel.channel_id,
        &channel.unit,
        ChannelKind::SampledSeries,
        channel.source_file_kind,
    );
    sha256(
        issues,
        &format!("{path}.source_sha256"),
        &channel.source_sha256,
    );
    sha256(issues, &format!("{path}.sha256"), &channel.sha256);
    validate_waveform_encoding(issues, &format!("{path}.encoding"), &channel.encoding);
    if !channel.sample_rate_hz.is_finite() || channel.sample_rate_hz <= 0.0 {
        issue(
            issues,
            &format!("{path}.sample_rate_hz"),
            "sample rate must be finite and greater than zero",
        );
    }
    if channel.sample_count == 0 {
        issue(
            issues,
            &format!("{path}.sample_count"),
            "an emitted waveform channel must contain at least one sample",
        );
    }
    json_safe_unsigned(
        issues,
        &format!("{path}.sample_count"),
        channel.sample_count,
    );
    json_safe_signed(
        issues,
        &format!("{path}.start_offset_milliseconds"),
        channel.start_offset_milliseconds,
    );
    if channel.start_offset_milliseconds < 0 {
        issue(
            issues,
            &format!("{path}.start_offset_milliseconds"),
            "must be nonnegative",
        );
    }
    if let Ok(duration) = channel.encoding.record_duration_decimal.parse::<f64>() {
        let encoded_rate = f64::from(channel.encoding.samples_per_record) / duration;
        if encoded_rate.is_finite()
            && (encoded_rate - channel.sample_rate_hz).abs()
                > DEFAULT_FLOAT_TOLERANCES.waveform_sample_rate_hz_abs
        {
            issue(
                issues,
                &format!("{path}.sample_rate_hz"),
                "does not match encoding samples_per_record / record_duration_decimal",
            );
        }
    }
    let expected_preview = usize::try_from(channel.sample_count)
        .unwrap_or(usize::MAX)
        .min(WAVEFORM_PREVIEW_SAMPLES);
    for (field, samples) in [
        ("head_samples", &channel.head_samples),
        ("tail_samples", &channel.tail_samples),
    ] {
        if samples.len() != expected_preview {
            issue(
                issues,
                &format!("{path}.{field}"),
                &format!(
                    "must contain min(sample_count, {WAVEFORM_PREVIEW_SAMPLES}) = {expected_preview} samples"
                ),
            );
        }
        for (sample_index, sample) in samples.iter().copied().enumerate() {
            finite(issues, &format!("{path}.{field}[{sample_index}]"), sample);
        }
    }
    if channel.head_samples.len() == expected_preview
        && channel.tail_samples.len() == expected_preview
    {
        let sample_count = usize::try_from(channel.sample_count).unwrap_or(usize::MAX);
        let tail_start = sample_count.saturating_sub(expected_preview);
        let overlap = expected_preview.saturating_sub(tail_start);
        for overlap_index in 0..overlap {
            if channel.head_samples[tail_start + overlap_index]
                != channel.tail_samples[overlap_index]
            {
                issue(
                    issues,
                    &format!("{path}.tail_samples[{overlap_index}]"),
                    "must equal the overlapping head sample",
                );
            }
        }
    }

    if channel.segments.is_empty() {
        issue(
            issues,
            &format!("{path}.segments"),
            "at least one waveform segment is required",
        );
    }
    let mut expected_start_sample = 0_u64;
    let mut previous_segment: Option<&WaveformSegment> = None;
    for (segment_index, segment) in channel.segments.iter().enumerate() {
        let segment_path = format!("{path}.segments[{segment_index}]");
        if segment.sequence != segment_index as u64 {
            issue(
                issues,
                &format!("{segment_path}.sequence"),
                "segment sequence must be zero-based and contiguous",
            );
        }
        json_safe_unsigned(
            issues,
            &format!("{segment_path}.sequence"),
            segment.sequence,
        );
        if segment.start_sample != expected_start_sample {
            issue(
                issues,
                &format!("{segment_path}.start_sample"),
                "must exactly continue the emitted sample stream",
            );
        }
        json_safe_unsigned(
            issues,
            &format!("{segment_path}.start_sample"),
            segment.start_sample,
        );
        if segment.sample_count == 0 {
            issue(
                issues,
                &format!("{segment_path}.sample_count"),
                "must be greater than zero",
            );
        }
        json_safe_unsigned(
            issues,
            &format!("{segment_path}.sample_count"),
            segment.sample_count,
        );
        json_safe_signed(
            issues,
            &format!("{segment_path}.start_offset_milliseconds"),
            segment.start_offset_milliseconds,
        );
        if segment.start_offset_milliseconds < 0 {
            issue(
                issues,
                &format!("{segment_path}.start_offset_milliseconds"),
                "must be nonnegative",
            );
        }
        sha256(
            issues,
            &format!("{segment_path}.source_sha256"),
            &segment.source_sha256,
        );
        if segment_index == 0
            && segment.start_offset_milliseconds != channel.start_offset_milliseconds
        {
            issue(
                issues,
                &format!("{segment_path}.start_offset_milliseconds"),
                "must equal the channel start offset for the first segment",
            );
        }
        if previous_segment.is_some_and(|previous| {
            !segment_fits_before(
                previous,
                segment.start_offset_milliseconds,
                &channel.encoding,
            )
            .unwrap_or(false)
        }) {
            issue(
                issues,
                &segment_path,
                "waveform segments must not overlap in time",
            );
        }
        if session_duration_milliseconds.is_some_and(|duration| {
            !segment_fits_before(segment, duration, &channel.encoding).unwrap_or(false)
        }) {
            issue(
                issues,
                &segment_path,
                "waveform segment extends beyond the session end",
            );
        }
        previous_segment = Some(segment);
        expected_start_sample = match expected_start_sample.checked_add(segment.sample_count) {
            Some(value) => value,
            None => {
                issue(issues, &segment_path, "sample range overflows u64");
                expected_start_sample
            }
        };
    }
    if expected_start_sample != channel.sample_count {
        issue(
            issues,
            &format!("{path}.segments"),
            "segment sample counts must exactly cover sample_count",
        );
    }
}

fn segment_fits_before(
    segment: &WaveformSegment,
    boundary_offset_milliseconds: i64,
    encoding: &WaveformEncoding,
) -> Option<bool> {
    let delta_milliseconds =
        boundary_offset_milliseconds.checked_sub(segment.start_offset_milliseconds)?;
    if delta_milliseconds < 0 {
        return Some(false);
    }
    let (duration_numerator, duration_denominator) =
        canonical_positive_decimal_ratio(&encoding.record_duration_decimal)?;
    let left = i128::from(segment.sample_count)
        .checked_mul(duration_numerator)?
        .checked_mul(1_000)?;
    let right = i128::from(delta_milliseconds)
        .checked_mul(i128::from(encoding.samples_per_record))?
        .checked_mul(duration_denominator)?;
    Some(left <= right)
}

fn validate_registered_channel(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    channel_id: &str,
    unit: &str,
    expected_kind: ChannelKind,
    source_file_kind: ResmedFileKind,
) {
    let Some(definition) = by_stable_key(channel_id) else {
        issue(
            issues,
            &format!("{path}.channel_id"),
            "is not present in the pinned OPAP channel registry",
        );
        return;
    };
    if definition.kind != expected_kind {
        issue(
            issues,
            &format!("{path}.channel_id"),
            "has the wrong registered storage kind",
        );
    }
    if definition.unit.symbol() != unit {
        issue(
            issues,
            &format!("{path}.unit"),
            "does not match the registered canonical unit",
        );
    }
    if !definition
        .resmed_signals
        .iter()
        .any(|signal| signal.file == source_file_kind)
    {
        issue(
            issues,
            &format!("{path}.source_file_kind"),
            "is not registered for this channel",
        );
    }
}

fn validate_waveform_encoding(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    encoding: &WaveformEncoding,
) {
    exact_contract(
        issues,
        &format!("{path}.kind"),
        &encoding.kind,
        WAVEFORM_DIGEST_ENCODING,
    );
    if encoding.digital_min >= encoding.digital_max {
        issue(
            issues,
            &format!("{path}.digital_min"),
            "must be less than digital_max",
        );
    }
    if i16::try_from(encoding.digital_min).is_err() {
        issue(
            issues,
            &format!("{path}.digital_min"),
            "must fit the EDF signed 16-bit digital range",
        );
    }
    if i16::try_from(encoding.digital_max).is_err() {
        issue(
            issues,
            &format!("{path}.digital_max"),
            "must fit the EDF signed 16-bit digital range",
        );
    }
    let physical_min = positive_or_negative_decimal(
        issues,
        &format!("{path}.physical_min_decimal"),
        &encoding.physical_min_decimal,
    );
    let physical_max = positive_or_negative_decimal(
        issues,
        &format!("{path}.physical_max_decimal"),
        &encoding.physical_max_decimal,
    );
    if let (Some(min), Some(max)) = (physical_min, physical_max)
        && min >= max
    {
        issue(
            issues,
            &format!("{path}.physical_min_decimal"),
            "must represent a value less than physical_max_decimal",
        );
    }
    if encoding.samples_per_record == 0 {
        issue(
            issues,
            &format!("{path}.samples_per_record"),
            "must be greater than zero",
        );
    }
    match positive_or_negative_decimal(
        issues,
        &format!("{path}.record_duration_decimal"),
        &encoding.record_duration_decimal,
    ) {
        Some(value) if value > 0.0 => {}
        Some(_) => issue(
            issues,
            &format!("{path}.record_duration_decimal"),
            "must represent a value greater than zero",
        ),
        None => {}
    }
}

fn positive_or_negative_decimal(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    value: &str,
) -> Option<f64> {
    if value.len() > 8 || !canonical_plain_decimal(value) {
        issue(
            issues,
            path,
            "must be an at-most-8-byte canonical EDF decimal without exponent, leading zero, trailing fractional zero, or negative zero",
        );
        return None;
    }
    let parsed = value.parse::<f64>().ok()?;
    if !parsed.is_finite() {
        issue(issues, path, "must represent a finite value");
        None
    } else {
        Some(parsed)
    }
}

fn canonical_plain_decimal(value: &str) -> bool {
    let (negative, unsigned) = match value.strip_prefix('-') {
        Some(unsigned) => (true, unsigned),
        None => (false, value),
    };
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    if whole.is_empty()
        || !ascii_digits(whole.as_bytes())
        || (whole.len() > 1 && whole.starts_with('0'))
    {
        return false;
    }
    if let Some(fraction) = fraction
        && (fraction.is_empty() || !ascii_digits(fraction.as_bytes()) || fraction.ends_with('0'))
    {
        return false;
    }
    if negative
        && whole == "0"
        && fraction.is_none_or(|fraction| fraction.bytes().all(|b| b == b'0'))
    {
        return false;
    }
    true
}

fn canonical_positive_decimal_ratio(value: &str) -> Option<(i128, i128)> {
    if !canonical_plain_decimal(value) || value.starts_with('-') {
        return None;
    }
    let (whole, fraction) = value
        .split_once('.')
        .map_or((value, ""), |(whole, fraction)| (whole, fraction));
    let mut digits = String::with_capacity(whole.len() + fraction.len());
    digits.push_str(whole);
    digits.push_str(fraction);
    let numerator = digits.parse::<i128>().ok()?;
    if numerator <= 0 {
        return None;
    }
    let denominator = 10_i128.checked_pow(u32::try_from(fraction.len()).ok()?)?;
    Some((numerator, denominator))
}

fn issue(issues: &mut Vec<ValidationIssue>, path: &str, message: &str) {
    issues.push(ValidationIssue {
        path: path.to_owned(),
        message: message.to_owned(),
    });
}

fn exact_contract(issues: &mut Vec<ValidationIssue>, path: &str, actual: &str, expected: &str) {
    if actual != expected {
        issue(issues, path, "does not match the required pinned value");
    }
}

fn non_empty(issues: &mut Vec<ValidationIssue>, path: &str, value: &str) {
    if value.trim().is_empty() {
        issue(issues, path, "must not be empty");
    }
}

fn canonical_identifier(issues: &mut Vec<ValidationIssue>, path: &str, value: &str) {
    let mut bytes = value.bytes();
    let valid_start = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    let valid_rest = bytes.all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
    });
    if !valid_start || !valid_rest {
        issue(
            issues,
            path,
            "must use only lowercase ASCII letters, digits, underscore, hyphen, or dot",
        );
    }
}

fn sha256(issues: &mut Vec<ValidationIssue>, path: &str, value: &str) {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        issue(
            issues,
            path,
            "must be a 64-character lowercase SHA-256 hex digest",
        );
    }
}

fn git_revision(issues: &mut Vec<ValidationIssue>, path: &str, value: &str) {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        issue(
            issues,
            path,
            "must be a full 40-character lowercase Git revision",
        );
    }
}

fn count_matches(issues: &mut Vec<ValidationIssue>, path: &str, declared: u64, actual: usize) {
    if usize::try_from(declared).ok() != Some(actual) {
        issue(issues, path, "declared count does not match array length");
    }
}

fn json_safe_unsigned(issues: &mut Vec<ValidationIssue>, path: &str, value: u64) {
    if value > JSON_SAFE_INTEGER {
        issue(
            issues,
            path,
            "must fit the cross-language JSON safe-integer range",
        );
    }
}

fn json_safe_signed(issues: &mut Vec<ValidationIssue>, path: &str, value: i64) {
    let limit = JSON_SAFE_INTEGER as i64;
    if !(-limit..=limit).contains(&value) {
        issue(
            issues,
            path,
            "must fit the cross-language JSON safe-integer range",
        );
    }
}

fn sorted_unique_by<'a, T, F>(
    issues: &mut Vec<ValidationIssue>,
    path: &str,
    values: &'a [T],
    key: F,
) where
    F: Fn(&'a T) -> &'a str,
{
    for index in 1..values.len() {
        let previous = key(&values[index - 1]);
        let current = key(&values[index]);
        if previous >= current {
            issue(
                issues,
                path,
                "entries must be uniquely sorted by their canonical identifier",
            );
            break;
        }
    }
}

fn finite(issues: &mut Vec<ValidationIssue>, path: &str, value: f64) {
    if !value.is_finite() {
        issue(issues, path, "must be finite");
    }
}

fn utc_timestamp(issues: &mut Vec<ValidationIssue>, path: &str, value: &str) -> Option<i64> {
    let parsed = parse_iso_timestamp(value, true);
    if parsed.is_none() {
        issue(
            issues,
            path,
            "must be a canonical YYYY-MM-DDTHH:MM:SS.mmmZ UTC timestamp",
        );
    }
    parsed
}

fn local_timestamp(issues: &mut Vec<ValidationIssue>, path: &str, value: &str) -> Option<i64> {
    let parsed = parse_iso_timestamp(value, false);
    if parsed.is_none() {
        issue(
            issues,
            path,
            "must be a canonical YYYY-MM-DDTHH:MM:SS.mmm local timestamp with no suffix",
        );
    }
    parsed
}

fn parse_iso_timestamp(value: &str, utc: bool) -> Option<i64> {
    let body = if utc {
        value.strip_suffix('Z')?
    } else {
        if value.ends_with('Z') {
            return None;
        }
        value
    };
    let (date, time) = body.split_once('T')?;
    if time.len() != 12 || time.contains('T') || date.len() != 10 {
        return None;
    }
    let date = date.as_bytes();
    if date[4] != b'-'
        || date[7] != b'-'
        || !ascii_digits(&date[0..4])
        || !ascii_digits(&date[5..7])
        || !ascii_digits(&date[8..10])
    {
        return None;
    }
    let year = decimal(&date[0..4]);
    let month = decimal(&date[5..7]);
    let day = decimal(&date[8..10]);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > max_day {
        return None;
    }

    let time = time.as_bytes();
    if time[2] != b':'
        || time[5] != b':'
        || time[8] != b'.'
        || !ascii_digits(&time[0..2])
        || !ascii_digits(&time[3..5])
        || !ascii_digits(&time[6..8])
        || !ascii_digits(&time[9..12])
    {
        return None;
    }
    let hour = decimal(&time[0..2]);
    let minute = decimal(&time[3..5]);
    let second = decimal(&time[6..8]);
    let millisecond = decimal(&time[9..12]);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
    Some(
        (days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
            * 1_000
            + i64::from(millisecond),
    )
}

fn ascii_digits(bytes: &[u8]) -> bool {
    bytes.iter().all(u8::is_ascii_digit)
}

fn decimal(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_from_civil(mut year: i64, month: i64, day: i64) -> i64 {
    year -= i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DifferenceKind {
    Missing,
    Unexpected,
    ExactMismatch,
    FloatOutOfTolerance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Difference {
    pub path: String,
    pub kind: DifferenceKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComparisonReport {
    pub compatible: bool,
    pub differences: Vec<Difference>,
}

pub fn compare(
    expected: &CompatibilityManifest,
    actual: &CompatibilityManifest,
    tolerances: FloatTolerances,
) -> Result<ComparisonReport, HarnessError> {
    let tolerance_issues = validate_tolerances(tolerances);
    if !tolerance_issues.is_empty() {
        return Err(HarnessError::InvalidTolerances {
            issues: tolerance_issues,
        });
    }
    for (label, manifest) in [("expected", expected), ("actual", actual)] {
        let issues = validate(manifest);
        if !issues.is_empty() {
            return Err(HarnessError::InvalidManifest {
                label: label.to_owned(),
                issues,
            });
        }
    }
    for (label, actual_role, required_role) in [
        ("expected", expected.producer.role, ProducerRole::Oracle),
        ("actual", actual.producer.role, ProducerRole::Subject),
    ] {
        if actual_role != required_role {
            return Err(HarnessError::InvalidManifest {
                label: label.to_owned(),
                issues: vec![ValidationIssue {
                    path: "$.producer.role".to_owned(),
                    message: format!(
                        "differential comparison requires role {required_role:?} in this position"
                    ),
                }],
            });
        }
    }

    let mut differences = Vec::new();
    compare_exact(
        &mut differences,
        "$.schema_version",
        &expected.schema_version,
        &actual.schema_version,
    );
    compare_oracle(&mut differences, &expected.oracle, &actual.oracle);
    compare_fixture(&mut differences, &expected.fixture, &actual.fixture);
    compare_machine(&mut differences, &expected.machine, &actual.machine);

    compare_keyed(
        &mut differences,
        "$.sessions",
        &expected.sessions,
        &actual.sessions,
        |session| session.source_id_sha256.clone(),
        |differences, path, expected, actual| {
            compare_session(differences, path, expected, actual, tolerances)
        },
    );

    Ok(ComparisonReport {
        compatible: differences.is_empty(),
        differences,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveformDigestError {
    SampleLengthMismatch,
    DeclaredSampleCountMismatch,
    InvalidPlacement,
    InvalidSegmentLayout { index: usize },
    InvalidSourceDigest { segment_index: Option<usize> },
    NonFinitePhysicalSample { index: usize },
}

impl fmt::Display for WaveformDigestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SampleLengthMismatch => {
                formatter.write_str("decoded digital and emitted physical waveform lengths differ")
            }
            Self::DeclaredSampleCountMismatch => {
                formatter.write_str("declared waveform sample count does not match emitted streams")
            }
            Self::InvalidPlacement => {
                formatter.write_str("waveform start placement is negative or unrepresented")
            }
            Self::InvalidSegmentLayout { index } => {
                write!(
                    formatter,
                    "waveform segment layout is invalid at index {index}"
                )
            }
            Self::InvalidSourceDigest {
                segment_index: None,
            } => formatter.write_str("waveform source digest is not lowercase SHA-256"),
            Self::InvalidSourceDigest {
                segment_index: Some(index),
            } => write!(
                formatter,
                "waveform segment source digest at index {index} is not lowercase SHA-256"
            ),
            Self::NonFinitePhysicalSample { index } => write!(
                formatter,
                "emitted physical waveform sample at index {index} is not finite"
            ),
        }
    }
}

impl std::error::Error for WaveformDigestError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceDigestError {
    NonCanonicalPath { index: usize },
    UnsortedOrDuplicatePath { index: usize },
    LengthOverflow,
}

impl fmt::Display for SourceDigestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonCanonicalPath { index } => {
                write!(
                    formatter,
                    "source-tree path at index {index} is not canonical"
                )
            }
            Self::UnsortedOrDuplicatePath { index } => write!(
                formatter,
                "source-tree path at index {index} is not strictly sorted"
            ),
            Self::LengthOverflow => formatter.write_str("source digest input length overflows"),
        }
    }
}

impl std::error::Error for SourceDigestError {}

/// Hashes raw source records in their exact producer-selected order.
pub fn record_stream_sha256<'a>(
    records: impl IntoIterator<Item = &'a [u8]>,
) -> Result<String, SourceDigestError> {
    let records = records.into_iter().collect::<Vec<_>>();
    let count = u64::try_from(records.len()).map_err(|_| SourceDigestError::LengthOverflow)?;
    let mut digest = Sha256::new();
    digest.update(b"opap-source-record-stream-v1\0");
    digest.update(count.to_be_bytes());
    for record in records {
        let length = u64::try_from(record.len()).map_err(|_| SourceDigestError::LengthOverflow)?;
        digest.update(length.to_be_bytes());
        digest.update(record);
    }
    Ok(encode_sha256(digest.finalize()))
}

/// Hashes a strictly sorted synthetic/card source tree. Callers must reject
/// symlinks and provide canonical `/`-separated relative paths.
pub fn source_tree_sha256<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<String, SourceDigestError> {
    let entries = entries.into_iter().collect::<Vec<_>>();
    let count = u64::try_from(entries.len()).map_err(|_| SourceDigestError::LengthOverflow)?;
    let mut digest = Sha256::new();
    digest.update(b"opap-source-tree-v1\0");
    digest.update(count.to_be_bytes());
    let mut previous_path = None;
    for (index, (path, contents)) in entries.into_iter().enumerate() {
        if !canonical_relative_path(path) {
            return Err(SourceDigestError::NonCanonicalPath { index });
        }
        if previous_path.is_some_and(|previous| previous >= path) {
            return Err(SourceDigestError::UnsortedOrDuplicatePath { index });
        }
        let path_length =
            u32::try_from(path.len()).map_err(|_| SourceDigestError::LengthOverflow)?;
        let content_length =
            u64::try_from(contents.len()).map_err(|_| SourceDigestError::LengthOverflow)?;
        digest.update(path_length.to_be_bytes());
        digest.update(path.as_bytes());
        digest.update(content_length.to_be_bytes());
        digest.update(contents);
        previous_path = Some(path);
    }
    Ok(encode_sha256(digest.finalize()))
}

fn canonical_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

pub struct WaveformDigestInput<'a> {
    pub channel_id: &'a str,
    pub source_file_kind: ResmedFileKind,
    pub unit: &'a str,
    pub source_sha256: &'a str,
    pub declared_sample_count: u64,
    pub start_offset_milliseconds: i64,
    pub segments: &'a [WaveformSegment],
    pub encoding: &'a WaveformEncoding,
    pub decoded_samples: &'a [i16],
    pub emitted_physical_samples: &'a [f32],
}

/// Hashes exact placement, segment provenance, decoded EDF digital samples,
/// and the final importer-emitted `f32` physical sample stream.
pub fn waveform_semantic_sha256(
    input: &WaveformDigestInput<'_>,
) -> Result<String, WaveformDigestError> {
    if input.decoded_samples.len() != input.emitted_physical_samples.len() {
        return Err(WaveformDigestError::SampleLengthMismatch);
    }
    if usize::try_from(input.declared_sample_count).ok() != Some(input.decoded_samples.len()) {
        return Err(WaveformDigestError::DeclaredSampleCountMismatch);
    }
    if input.declared_sample_count > JSON_SAFE_INTEGER || input.start_offset_milliseconds < 0 {
        return Err(WaveformDigestError::InvalidPlacement);
    }
    let mut expected_start_sample = 0_u64;
    let mut previous_segment = None;
    for (index, segment) in input.segments.iter().enumerate() {
        if segment.sequence != index as u64
            || segment.start_sample != expected_start_sample
            || segment.sample_count == 0
            || segment.start_offset_milliseconds < 0
            || (index == 0 && segment.start_offset_milliseconds != input.start_offset_milliseconds)
        {
            return Err(WaveformDigestError::InvalidSegmentLayout { index });
        }
        if previous_segment.is_some_and(|previous| {
            !segment_fits_before(previous, segment.start_offset_milliseconds, input.encoding)
                .unwrap_or(false)
        }) {
            return Err(WaveformDigestError::InvalidSegmentLayout { index });
        }
        expected_start_sample = expected_start_sample
            .checked_add(segment.sample_count)
            .ok_or(WaveformDigestError::InvalidSegmentLayout { index })?;
        previous_segment = Some(segment);
    }
    if input.segments.is_empty() || expected_start_sample != input.declared_sample_count {
        return Err(WaveformDigestError::InvalidPlacement);
    }
    let source_digest =
        decode_sha256(input.source_sha256).ok_or(WaveformDigestError::InvalidSourceDigest {
            segment_index: None,
        })?;
    let mut digest = Sha256::new();
    digest.update(b"opap-waveform-semantic-edf-i16-f32-segments-v2\0");
    digest_string(&mut digest, input.channel_id);
    digest_string(&mut digest, resmed_file_kind_name(input.source_file_kind));
    digest_string(&mut digest, input.unit);
    digest.update(source_digest);
    digest.update(input.start_offset_milliseconds.to_be_bytes());
    digest.update(input.declared_sample_count.to_be_bytes());
    digest_string(&mut digest, &input.encoding.kind);
    digest.update(input.encoding.digital_min.to_be_bytes());
    digest.update(input.encoding.digital_max.to_be_bytes());
    digest_string(&mut digest, &input.encoding.physical_min_decimal);
    digest_string(&mut digest, &input.encoding.physical_max_decimal);
    digest.update(input.encoding.samples_per_record.to_be_bytes());
    digest_string(&mut digest, &input.encoding.record_duration_decimal);
    digest.update((input.segments.len() as u64).to_be_bytes());
    for (index, segment) in input.segments.iter().enumerate() {
        digest.update(segment.sequence.to_be_bytes());
        digest.update(segment.start_sample.to_be_bytes());
        digest.update(segment.sample_count.to_be_bytes());
        digest.update(segment.start_offset_milliseconds.to_be_bytes());
        let segment_digest = decode_sha256(&segment.source_sha256).ok_or(
            WaveformDigestError::InvalidSourceDigest {
                segment_index: Some(index),
            },
        )?;
        digest.update(segment_digest);
    }
    digest.update((input.decoded_samples.len() as u64).to_be_bytes());
    for digital in input.decoded_samples {
        digest.update(digital.to_le_bytes());
    }
    digest.update((input.emitted_physical_samples.len() as u64).to_be_bytes());
    for (index, physical) in input.emitted_physical_samples.iter().enumerate() {
        if !physical.is_finite() {
            return Err(WaveformDigestError::NonFinitePhysicalSample { index });
        }
        digest.update(physical.to_bits().to_be_bytes());
    }
    Ok(encode_sha256(digest.finalize()))
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        bytes[index] = (high << 4) | low;
    }
    Some(bytes)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn resmed_file_kind_name(kind: ResmedFileKind) -> &'static str {
    match kind {
        ResmedFileKind::Eve => "eve",
        ResmedFileKind::Brp => "brp",
        ResmedFileKind::Pld => "pld",
        ResmedFileKind::Csl => "csl",
        ResmedFileKind::Str => "str",
    }
}

/// Hashes the exact ordered session-slice identity, state, and placement.
pub fn slice_collection_sha256(slices: &[SessionSlice]) -> String {
    let entries = slices
        .iter()
        .map(|slice| {
            serde_json::json!([
                slice.sequence,
                slice.source_id_sha256,
                match slice.status {
                    SliceStatus::MaskOn => "mask_on",
                    SliceStatus::MaskOff => "mask_off",
                    SliceStatus::EquipmentOff => "equipment_off",
                },
                slice.start_offset_milliseconds,
                slice.end_offset_milliseconds
            ])
        })
        .collect::<Vec<_>>();
    canonical_json_sha256(&serde_json::json!(["opap-slice-collection-v1", entries]))
}

/// Hashes an ordered event-channel digest collection using the documented JCS
/// array preimage. Inputs must already be in canonical channel order.
pub fn event_collection_sha256<'a>(
    channels: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let entries: Vec<[&str; 2]> = channels
        .into_iter()
        .map(|(channel_id, sha256)| [channel_id, sha256])
        .collect();
    canonical_json_sha256(&serde_json::json!(["opap-event-collection-v1", entries]))
}

/// Hashes an ordered waveform collection of channel, raw-source digest, and
/// semantic digest triples. Inputs must already be in canonical channel order.
pub fn waveform_collection_sha256<'a>(
    channels: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
) -> String {
    let entries: Vec<[&str; 3]> = channels
        .into_iter()
        .map(|(channel_id, source_sha256, semantic_sha256)| {
            [channel_id, source_sha256, semantic_sha256]
        })
        .collect();
    canonical_json_sha256(&serde_json::json!(["opap-waveform-collection-v1", entries]))
}

/// Hashes every exact child commitment that defines a complete session.
pub fn session_aggregate_sha256(
    source_id_sha256: &str,
    source_sha256: &str,
    slices_sha256: &str,
    summary_source_sha256: &str,
    settings_sha256: &str,
    events_sha256: &str,
    waveforms_sha256: &str,
) -> String {
    canonical_json_sha256(&serde_json::json!([
        "opap-session-aggregate-v1",
        source_id_sha256,
        source_sha256,
        slices_sha256,
        summary_source_sha256,
        settings_sha256,
        events_sha256,
        waveforms_sha256
    ]))
}

fn canonical_json_sha256(value: &serde_json::Value) -> String {
    // These preimages contain arrays of ASCII strings and JSON-safe integers
    // only. Compact serde_json serialization is therefore byte-for-byte JCS.
    let bytes = serde_json::to_vec(value).expect("JSON Value serialization cannot fail");
    let mut digest = Sha256::new();
    digest.update(bytes);
    encode_sha256(digest.finalize())
}

fn encode_sha256(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut encoded = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

fn digest_string(digest: &mut Sha256, value: &str) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
}

fn validate_tolerances(tolerances: FloatTolerances) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for (name, value) in tolerances.named() {
        if !value.is_finite() || value < 0.0 {
            issue(
                &mut issues,
                &format!("$.tolerances.{name}"),
                "must be finite and nonnegative",
            );
        }
    }
    issues
}

fn compare_oracle(
    differences: &mut Vec<Difference>,
    expected: &OracleIdentity,
    actual: &OracleIdentity,
) {
    compare_exact(differences, "$.oracle.name", &expected.name, &actual.name);
    compare_exact(
        differences,
        "$.oracle.repository",
        &expected.repository,
        &actual.repository,
    );
    compare_exact(
        differences,
        "$.oracle.revision",
        &expected.revision,
        &actual.revision,
    );
    compare_exact(
        differences,
        "$.oracle.export_schema_version",
        &expected.export_schema_version,
        &actual.export_schema_version,
    );
}

fn compare_fixture(
    differences: &mut Vec<Difference>,
    expected: &FixtureIdentity,
    actual: &FixtureIdentity,
) {
    compare_exact(
        differences,
        "$.fixture.case_id",
        &expected.case_id,
        &actual.case_id,
    );
    compare_exact(
        differences,
        "$.fixture.source_sha256",
        &expected.source_sha256,
        &actual.source_sha256,
    );
    compare_exact(
        differences,
        "$.fixture.synthetic",
        &expected.synthetic,
        &actual.synthetic,
    );
}

fn compare_machine(differences: &mut Vec<Difference>, expected: &Machine, actual: &Machine) {
    for (field, expected, actual) in [
        ("manufacturer", &expected.manufacturer, &actual.manufacturer),
        ("model", &expected.model, &actual.model),
        ("model_number", &expected.model_number, &actual.model_number),
        (
            "serial_number",
            &expected.serial_number,
            &actual.serial_number,
        ),
        ("firmware", &expected.firmware, &actual.firmware),
        ("sha256", &expected.sha256, &actual.sha256),
    ] {
        compare_exact(differences, &format!("$.machine.{field}"), expected, actual);
    }
}

fn compare_session(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &Session,
    actual: &Session,
    tolerances: FloatTolerances,
) {
    compare_exact(
        differences,
        &format!("{path}.session_id"),
        &expected.session_id,
        &actual.session_id,
    );
    compare_exact(
        differences,
        &format!("{path}.source_id_sha256"),
        &expected.source_id_sha256,
        &actual.source_id_sha256,
    );
    compare_exact(
        differences,
        &format!("{path}.source_sha256"),
        &expected.source_sha256,
        &actual.source_sha256,
    );
    compare_exact(
        differences,
        &format!("{path}.sha256"),
        &expected.sha256,
        &actual.sha256,
    );
    compare_time(
        differences,
        &format!("{path}.time"),
        &expected.time,
        &actual.time,
    );

    compare_slices(
        differences,
        &format!("{path}.slices"),
        &expected.slices,
        &actual.slices,
    );
    compare_summary(
        differences,
        &format!("{path}.summary"),
        &expected.summary,
        &actual.summary,
        tolerances,
    );

    compare_exact(
        differences,
        &format!("{path}.settings.count"),
        &expected.settings.count,
        &actual.settings.count,
    );
    compare_exact(
        differences,
        &format!("{path}.settings.sha256"),
        &expected.settings.sha256,
        &actual.settings.sha256,
    );
    compare_keyed(
        differences,
        &format!("{path}.settings.items"),
        &expected.settings.items,
        &actual.settings.items,
        |setting| setting.key.clone(),
        |differences, path, expected, actual| {
            compare_exact(
                differences,
                &format!("{path}.unit"),
                &expected.unit,
                &actual.unit,
            );
            compare_setting_value(
                differences,
                &format!("{path}.value"),
                &expected.value,
                &actual.value,
                tolerances,
            );
        },
    );

    compare_exact(
        differences,
        &format!("{path}.events.channel_count"),
        &expected.events.channel_count,
        &actual.events.channel_count,
    );
    compare_exact(
        differences,
        &format!("{path}.events.sha256"),
        &expected.events.sha256,
        &actual.events.sha256,
    );
    compare_keyed(
        differences,
        &format!("{path}.events.channels"),
        &expected.events.channels,
        &actual.events.channels,
        |channel| channel.channel_id.clone(),
        |differences, path, expected, actual| {
            compare_event_channel(differences, path, expected, actual, tolerances)
        },
    );

    compare_exact(
        differences,
        &format!("{path}.waveforms.channel_count"),
        &expected.waveforms.channel_count,
        &actual.waveforms.channel_count,
    );
    compare_exact(
        differences,
        &format!("{path}.waveforms.sha256"),
        &expected.waveforms.sha256,
        &actual.waveforms.sha256,
    );
    compare_keyed(
        differences,
        &format!("{path}.waveforms.channels"),
        &expected.waveforms.channels,
        &actual.waveforms.channels,
        |channel| channel.channel_id.clone(),
        |differences, path, expected, actual| {
            compare_waveform_channel(differences, path, expected, actual, tolerances)
        },
    );
}

fn compare_slices(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &SliceCollection,
    actual: &SliceCollection,
) {
    compare_exact(
        differences,
        &format!("{path}.count"),
        &expected.count,
        &actual.count,
    );
    compare_exact(
        differences,
        &format!("{path}.sha256"),
        &expected.sha256,
        &actual.sha256,
    );
    for (index, (expected, actual)) in expected.items.iter().zip(&actual.items).enumerate() {
        let item_path = format!("{path}.items[{index}]");
        compare_exact(
            differences,
            &format!("{item_path}.sequence"),
            &expected.sequence,
            &actual.sequence,
        );
        compare_exact(
            differences,
            &format!("{item_path}.source_id_sha256"),
            &expected.source_id_sha256,
            &actual.source_id_sha256,
        );
        compare_exact(
            differences,
            &format!("{item_path}.status"),
            &expected.status,
            &actual.status,
        );
        compare_exact(
            differences,
            &format!("{item_path}.start_offset_milliseconds"),
            &expected.start_offset_milliseconds,
            &actual.start_offset_milliseconds,
        );
        compare_exact(
            differences,
            &format!("{item_path}.end_offset_milliseconds"),
            &expected.end_offset_milliseconds,
            &actual.end_offset_milliseconds,
        );
    }
    compare_vec_length(
        differences,
        &format!("{path}.items"),
        expected.items.len(),
        actual.items.len(),
    );
}

fn compare_summary(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &SessionSummary,
    actual: &SessionSummary,
    tolerances: FloatTolerances,
) {
    compare_exact(
        differences,
        &format!("{path}.usage_milliseconds"),
        &expected.usage_milliseconds,
        &actual.usage_milliseconds,
    );
    compare_exact(
        differences,
        &format!("{path}.metric_count"),
        &expected.metric_count,
        &actual.metric_count,
    );
    compare_exact(
        differences,
        &format!("{path}.source_sha256"),
        &expected.source_sha256,
        &actual.source_sha256,
    );
    compare_keyed(
        differences,
        &format!("{path}.metrics"),
        &expected.metrics,
        &actual.metrics,
        |metric| metric.key.clone(),
        |differences, path, expected, actual| {
            compare_exact(
                differences,
                &format!("{path}.unit"),
                &expected.unit,
                &actual.unit,
            );
            compare_nullable_float(
                differences,
                &format!("{path}.value"),
                expected.value,
                actual.value,
                "summary_metric_abs",
                tolerances.summary_metric_abs,
            );
        },
    );
}

fn compare_time(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &SessionTime,
    actual: &SessionTime,
) {
    compare_exact(
        differences,
        &format!("{path}.start_utc"),
        &expected.start_utc,
        &actual.start_utc,
    );
    compare_exact(
        differences,
        &format!("{path}.end_utc"),
        &expected.end_utc,
        &actual.end_utc,
    );
    compare_exact(
        differences,
        &format!("{path}.start_local"),
        &expected.start_local,
        &actual.start_local,
    );
    compare_exact(
        differences,
        &format!("{path}.end_local"),
        &expected.end_local,
        &actual.end_local,
    );
    compare_exact(
        differences,
        &format!("{path}.start_utc_offset_seconds"),
        &expected.start_utc_offset_seconds,
        &actual.start_utc_offset_seconds,
    );
    compare_exact(
        differences,
        &format!("{path}.end_utc_offset_seconds"),
        &expected.end_utc_offset_seconds,
        &actual.end_utc_offset_seconds,
    );
    compare_exact(
        differences,
        &format!("{path}.timezone_basis"),
        &expected.timezone_basis,
        &actual.timezone_basis,
    );
    compare_exact(
        differences,
        &format!("{path}.start_clock_correction_milliseconds"),
        &expected.start_clock_correction_milliseconds,
        &actual.start_clock_correction_milliseconds,
    );
    compare_exact(
        differences,
        &format!("{path}.end_clock_correction_milliseconds"),
        &expected.end_clock_correction_milliseconds,
        &actual.end_clock_correction_milliseconds,
    );
}

fn compare_setting_value(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &SettingValue,
    actual: &SettingValue,
    tolerances: FloatTolerances,
) {
    match (expected, actual) {
        (SettingValue::Number(expected), SettingValue::Number(actual)) => compare_float(
            differences,
            path,
            *expected,
            *actual,
            "setting_number_abs",
            tolerances.setting_number_abs,
        ),
        (SettingValue::Integer(expected), SettingValue::Integer(actual)) => {
            compare_exact(differences, path, expected, actual)
        }
        (SettingValue::Boolean(expected), SettingValue::Boolean(actual)) => {
            compare_exact(differences, path, expected, actual)
        }
        (SettingValue::Text(expected), SettingValue::Text(actual)) => {
            compare_exact(differences, path, expected, actual)
        }
        _ => push_difference(
            differences,
            path,
            DifferenceKind::ExactMismatch,
            format!(
                "setting value types differ: expected {}, actual {}",
                setting_type(expected),
                setting_type(actual)
            ),
        ),
    }
}

fn setting_type(value: &SettingValue) -> &'static str {
    match value {
        SettingValue::Number(_) => "number",
        SettingValue::Integer(_) => "integer",
        SettingValue::Boolean(_) => "boolean",
        SettingValue::Text(_) => "text",
    }
}

fn compare_event_channel(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &EventChannel,
    actual: &EventChannel,
    tolerances: FloatTolerances,
) {
    compare_exact(
        differences,
        &format!("{path}.source_file_kind"),
        &expected.source_file_kind,
        &actual.source_file_kind,
    );
    compare_exact(
        differences,
        &format!("{path}.unit"),
        &expected.unit,
        &actual.unit,
    );
    compare_exact(
        differences,
        &format!("{path}.count"),
        &expected.count,
        &actual.count,
    );
    compare_exact(
        differences,
        &format!("{path}.sha256"),
        &expected.sha256,
        &actual.sha256,
    );
    for (index, (expected, actual)) in expected.items.iter().zip(&actual.items).enumerate() {
        let event_path = format!("{path}.items[{index}]");
        compare_exact(
            differences,
            &format!("{event_path}.sequence"),
            &expected.sequence,
            &actual.sequence,
        );
        compare_exact(
            differences,
            &format!("{event_path}.source_id_sha256"),
            &expected.source_id_sha256,
            &actual.source_id_sha256,
        );
        compare_exact(
            differences,
            &format!("{event_path}.offset_milliseconds"),
            &expected.offset_milliseconds,
            &actual.offset_milliseconds,
        );
        compare_exact(
            differences,
            &format!("{event_path}.duration_milliseconds"),
            &expected.duration_milliseconds,
            &actual.duration_milliseconds,
        );
        compare_nullable_float(
            differences,
            &format!("{event_path}.value"),
            expected.value,
            actual.value,
            "event_value_abs",
            tolerances.event_value_abs,
        );
    }
    compare_vec_length(
        differences,
        &format!("{path}.items"),
        expected.items.len(),
        actual.items.len(),
    );
}

fn compare_nullable_float(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: NullableNumber,
    actual: NullableNumber,
    tolerance_name: &str,
    tolerance: f64,
) {
    match (expected.0, actual.0) {
        (Some(expected), Some(actual)) => compare_float(
            differences,
            path,
            expected,
            actual,
            tolerance_name,
            tolerance,
        ),
        (None, None) => {}
        _ => push_difference(
            differences,
            path,
            DifferenceKind::ExactMismatch,
            "presence differs (values redacted)".to_owned(),
        ),
    }
}

fn compare_waveform_channel(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &WaveformChannel,
    actual: &WaveformChannel,
    tolerances: FloatTolerances,
) {
    compare_exact(
        differences,
        &format!("{path}.source_file_kind"),
        &expected.source_file_kind,
        &actual.source_file_kind,
    );
    compare_exact(
        differences,
        &format!("{path}.unit"),
        &expected.unit,
        &actual.unit,
    );
    compare_float(
        differences,
        &format!("{path}.sample_rate_hz"),
        expected.sample_rate_hz,
        actual.sample_rate_hz,
        "waveform_sample_rate_hz_abs",
        tolerances.waveform_sample_rate_hz_abs,
    );
    compare_exact(
        differences,
        &format!("{path}.sample_count"),
        &expected.sample_count,
        &actual.sample_count,
    );
    compare_exact(
        differences,
        &format!("{path}.start_offset_milliseconds"),
        &expected.start_offset_milliseconds,
        &actual.start_offset_milliseconds,
    );
    compare_exact(
        differences,
        &format!("{path}.source_sha256"),
        &expected.source_sha256,
        &actual.source_sha256,
    );
    compare_exact(
        differences,
        &format!("{path}.sha256"),
        &expected.sha256,
        &actual.sha256,
    );
    compare_waveform_encoding(
        differences,
        &format!("{path}.encoding"),
        &expected.encoding,
        &actual.encoding,
    );
    for (index, (expected, actual)) in expected.segments.iter().zip(&actual.segments).enumerate() {
        let segment_path = format!("{path}.segments[{index}]");
        compare_exact(
            differences,
            &format!("{segment_path}.sequence"),
            &expected.sequence,
            &actual.sequence,
        );
        compare_exact(
            differences,
            &format!("{segment_path}.start_sample"),
            &expected.start_sample,
            &actual.start_sample,
        );
        compare_exact(
            differences,
            &format!("{segment_path}.sample_count"),
            &expected.sample_count,
            &actual.sample_count,
        );
        compare_exact(
            differences,
            &format!("{segment_path}.start_offset_milliseconds"),
            &expected.start_offset_milliseconds,
            &actual.start_offset_milliseconds,
        );
        compare_exact(
            differences,
            &format!("{segment_path}.source_sha256"),
            &expected.source_sha256,
            &actual.source_sha256,
        );
    }
    compare_vec_length(
        differences,
        &format!("{path}.segments"),
        expected.segments.len(),
        actual.segments.len(),
    );
    compare_float_vec(
        differences,
        &format!("{path}.head_samples"),
        &expected.head_samples,
        &actual.head_samples,
        tolerances.waveform_preview_sample_abs,
    );
    compare_float_vec(
        differences,
        &format!("{path}.tail_samples"),
        &expected.tail_samples,
        &actual.tail_samples,
        tolerances.waveform_preview_sample_abs,
    );
}

fn compare_waveform_encoding(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &WaveformEncoding,
    actual: &WaveformEncoding,
) {
    compare_exact(
        differences,
        &format!("{path}.kind"),
        &expected.kind,
        &actual.kind,
    );
    compare_exact(
        differences,
        &format!("{path}.digital_min"),
        &expected.digital_min,
        &actual.digital_min,
    );
    compare_exact(
        differences,
        &format!("{path}.digital_max"),
        &expected.digital_max,
        &actual.digital_max,
    );
    compare_exact(
        differences,
        &format!("{path}.physical_min_decimal"),
        &expected.physical_min_decimal,
        &actual.physical_min_decimal,
    );
    compare_exact(
        differences,
        &format!("{path}.physical_max_decimal"),
        &expected.physical_max_decimal,
        &actual.physical_max_decimal,
    );
    compare_exact(
        differences,
        &format!("{path}.samples_per_record"),
        &expected.samples_per_record,
        &actual.samples_per_record,
    );
    compare_exact(
        differences,
        &format!("{path}.record_duration_decimal"),
        &expected.record_duration_decimal,
        &actual.record_duration_decimal,
    );
}

fn compare_float_vec(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &[f64],
    actual: &[f64],
    tolerance: f64,
) {
    for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
        compare_float(
            differences,
            &format!("{path}[{index}]"),
            *expected,
            *actual,
            "waveform_preview_sample_abs",
            tolerance,
        );
    }
    compare_vec_length(differences, path, expected.len(), actual.len());
}

fn compare_vec_length(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: usize,
    actual: usize,
) {
    if expected != actual {
        push_difference(
            differences,
            path,
            DifferenceKind::ExactMismatch,
            "array lengths differ (values redacted)".to_owned(),
        );
    }
}

fn compare_float(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: f64,
    actual: f64,
    tolerance_name: &str,
    tolerance: f64,
) {
    let delta = (expected - actual).abs();
    if !delta.is_finite() || delta > tolerance {
        push_difference(
            differences,
            path,
            DifferenceKind::FloatOutOfTolerance,
            format!("absolute difference exceeds {tolerance_name}={tolerance} (values redacted)"),
        );
    }
}

fn compare_exact<T: PartialEq>(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &T,
    actual: &T,
) {
    if expected != actual {
        push_difference(
            differences,
            path,
            DifferenceKind::ExactMismatch,
            "values differ (redacted)".to_owned(),
        );
    }
}

fn compare_keyed<T, F, C>(
    differences: &mut Vec<Difference>,
    path: &str,
    expected: &[T],
    actual: &[T],
    key: F,
    compare_value: C,
) where
    F: Fn(&T) -> String,
    C: Fn(&mut Vec<Difference>, &str, &T, &T),
{
    let expected_by_key: BTreeMap<String, (usize, &T)> = expected
        .iter()
        .enumerate()
        .map(|(index, value)| (key(value), (index, value)))
        .collect();
    let actual_by_key: BTreeMap<String, (usize, &T)> = actual
        .iter()
        .enumerate()
        .map(|(index, value)| (key(value), (index, value)))
        .collect();

    for (item_key, (expected_index, expected_value)) in &expected_by_key {
        let item_path = format!("{path}[expected:{expected_index}]");
        match actual_by_key.get(item_key) {
            Some((_, actual_value)) => {
                compare_value(differences, &item_path, expected_value, actual_value)
            }
            None => push_difference(
                differences,
                &item_path,
                DifferenceKind::Missing,
                "missing required entry (identifier redacted)".to_owned(),
            ),
        }
    }
    for (item_key, (actual_index, _)) in &actual_by_key {
        if !expected_by_key.contains_key(item_key) {
            push_difference(
                differences,
                &format!("{path}[actual:{actual_index}]"),
                DifferenceKind::Unexpected,
                "unexpected entry (identifier redacted)".to_owned(),
            );
        }
    }
}

fn push_difference(
    differences: &mut Vec<Difference>,
    path: &str,
    kind: DifferenceKind,
    message: String,
) {
    differences.push(Difference {
        path: path.to_owned(),
        kind,
        message,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_digest_file_kind_names_cover_all_resmed_families() {
        assert_eq!(resmed_file_kind_name(ResmedFileKind::Eve), "eve");
        assert_eq!(resmed_file_kind_name(ResmedFileKind::Brp), "brp");
        assert_eq!(resmed_file_kind_name(ResmedFileKind::Pld), "pld");
        assert_eq!(resmed_file_kind_name(ResmedFileKind::Csl), "csl");
        assert_eq!(resmed_file_kind_name(ResmedFileKind::Str), "str");
    }

    #[derive(Deserialize)]
    struct WaveformDigestVector {
        channel_id: String,
        source_file_kind: ResmedFileKind,
        unit: String,
        source_sha256: String,
        declared_sample_count: u64,
        start_offset_milliseconds: i64,
        segments: Vec<WaveformSegment>,
        encoding: WaveformEncoding,
        decoded_samples: Vec<i16>,
        emitted_physical_samples: Vec<f32>,
        sha256: String,
    }

    #[derive(Deserialize)]
    struct AggregateDigestVector {
        slices: Vec<SessionSlice>,
        slice_collection_sha256: String,
        event_channels: Vec<[String; 2]>,
        event_collection_sha256: String,
        waveform_channels: Vec<[String; 3]>,
        waveform_collection_sha256: String,
        session: AggregateSessionInput,
        session_sha256: String,
    }

    #[derive(Deserialize)]
    struct AggregateSessionInput {
        source_id_sha256: String,
        source_sha256: String,
        slices_sha256: String,
        summary_source_sha256: String,
        settings_sha256: String,
        events_sha256: String,
        waveforms_sha256: String,
    }

    #[derive(Deserialize)]
    struct SourceDigestVector {
        records_utf8: Vec<String>,
        record_stream_sha256: String,
        tree: Vec<SourceTreeVectorEntry>,
        source_tree_sha256: String,
    }

    #[derive(Deserialize)]
    struct SourceTreeVectorEntry {
        path: String,
        contents_utf8: String,
    }

    fn digest_vector(vector: &WaveformDigestVector) -> Result<String, WaveformDigestError> {
        waveform_semantic_sha256(&WaveformDigestInput {
            channel_id: &vector.channel_id,
            source_file_kind: vector.source_file_kind,
            unit: &vector.unit,
            source_sha256: &vector.source_sha256,
            declared_sample_count: vector.declared_sample_count,
            start_offset_milliseconds: vector.start_offset_milliseconds,
            segments: &vector.segments,
            encoding: &vector.encoding,
            decoded_samples: &vector.decoded_samples,
            emitted_physical_samples: &vector.emitted_physical_samples,
        })
    }

    fn synthetic() -> CompatibilityManifest {
        serde_json::from_str(include_str!("../tests/fixtures/synthetic-oscar.json")).unwrap()
    }

    fn synthetic_subject() -> CompatibilityManifest {
        serde_json::from_str(include_str!("../tests/fixtures/synthetic-opap.json")).unwrap()
    }

    fn refresh_aggregate_digests(session: &mut Session) {
        session.slices.sha256 = slice_collection_sha256(&session.slices.items);
        session.events.sha256 = event_collection_sha256(
            session
                .events
                .channels
                .iter()
                .map(|channel| (channel.channel_id.as_str(), channel.sha256.as_str())),
        );
        session.waveforms.sha256 =
            waveform_collection_sha256(session.waveforms.channels.iter().map(|channel| {
                (
                    channel.channel_id.as_str(),
                    channel.source_sha256.as_str(),
                    channel.sha256.as_str(),
                )
            }));
        session.sha256 = session_aggregate_sha256(
            &session.source_id_sha256,
            &session.source_sha256,
            &session.slices.sha256,
            &session.summary.source_sha256,
            &session.settings.sha256,
            &session.events.sha256,
            &session.waveforms.sha256,
        );
    }

    #[test]
    fn canonical_fixture_is_valid() {
        assert_eq!(validate(&synthetic()), Vec::new());
    }

    #[test]
    fn waveform_semantic_digest_matches_the_public_vector() {
        let vector: WaveformDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/waveform-digest-vector.json"
        ))
        .unwrap();
        let digest = digest_vector(&vector).unwrap();
        assert_eq!(digest, vector.sha256);
        assert_eq!(synthetic().sessions[0].waveforms.channels[0].sha256, digest);

        let mut digital_corruption = vector;
        digital_corruption.decoded_samples[20] += 1;
        assert_ne!(digest_vector(&digital_corruption).unwrap(), digest);

        let mut physical_corruption: WaveformDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/waveform-digest-vector.json"
        ))
        .unwrap();
        physical_corruption.emitted_physical_samples[20] += 0.5;
        assert_ne!(digest_vector(&physical_corruption).unwrap(), digest);

        let mut placement_corruption: WaveformDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/waveform-digest-vector.json"
        ))
        .unwrap();
        placement_corruption.segments[1].start_offset_milliseconds += 1;
        assert_ne!(digest_vector(&placement_corruption).unwrap(), digest);

        let mut metadata_corruption: WaveformDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/waveform-digest-vector.json"
        ))
        .unwrap();
        metadata_corruption.source_file_kind = ResmedFileKind::Pld;
        assert_ne!(digest_vector(&metadata_corruption).unwrap(), digest);

        let mut metadata_corruption: WaveformDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/waveform-digest-vector.json"
        ))
        .unwrap();
        metadata_corruption.start_offset_milliseconds += 1;
        metadata_corruption.segments[0].start_offset_milliseconds += 1;
        assert_ne!(digest_vector(&metadata_corruption).unwrap(), digest);

        let mut metadata_corruption: WaveformDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/waveform-digest-vector.json"
        ))
        .unwrap();
        metadata_corruption.segments[0].source_sha256 =
            "901984606f3ce77d8d065d8f0c65af09b086475b81f9d10a17c3f3831161645f".to_owned();
        assert_ne!(digest_vector(&metadata_corruption).unwrap(), digest);

        let mut invalid: WaveformDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/waveform-digest-vector.json"
        ))
        .unwrap();
        invalid.emitted_physical_samples.pop();
        assert_eq!(
            digest_vector(&invalid),
            Err(WaveformDigestError::SampleLengthMismatch)
        );

        let mut invalid: WaveformDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/waveform-digest-vector.json"
        ))
        .unwrap();
        invalid.emitted_physical_samples[20] = f32::NAN;
        assert_eq!(
            digest_vector(&invalid),
            Err(WaveformDigestError::NonFinitePhysicalSample { index: 20 })
        );
    }

    #[test]
    fn synthetic_differential_pair_compares_compatible() {
        let expected = synthetic();
        let actual = synthetic_subject();
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(report.compatible);
        assert!(report.differences.is_empty());
    }

    #[test]
    fn self_comparison_is_rejected_by_producer_role() {
        let oracle = synthetic();
        let error = compare(&oracle, &oracle, DEFAULT_FLOAT_TOLERANCES).unwrap_err();
        assert!(error.to_string().contains("requires role Subject"));
    }

    #[test]
    fn named_float_tolerance_is_applied() {
        let expected = synthetic();
        let mut actual = synthetic_subject();
        actual.sessions[0].waveforms.channels[0].head_samples[0] =
            expected.sessions[0].waveforms.channels[0].head_samples[0]
                + DEFAULT_FLOAT_TOLERANCES.waveform_preview_sample_abs / 2.0;
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(report.compatible);

        actual.sessions[0].waveforms.channels[0].head_samples[0] =
            expected.sessions[0].waveforms.channels[0].head_samples[0]
                + DEFAULT_FLOAT_TOLERANCES.waveform_preview_sample_abs * 2.0;
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert!(
            report.differences[0]
                .message
                .contains("waveform_preview_sample_abs")
        );
    }

    #[test]
    fn missing_session_never_passes() {
        let expected = synthetic();
        let mut actual = synthetic_subject();
        actual.sessions.clear();
        let error = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("at least one session is required")
        );
    }

    #[test]
    fn missing_channel_never_passes() {
        let expected = synthetic();
        let mut actual = synthetic_subject();
        actual.sessions[0].waveforms.channels.clear();
        actual.sessions[0].waveforms.channel_count = 0;
        refresh_aggregate_digests(&mut actual.sessions[0]);
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert!(
            report
                .differences
                .iter()
                .any(|difference| difference.kind == DifferenceKind::Missing)
        );
    }

    #[test]
    fn malformed_digest_is_rejected() {
        let mut manifest = synthetic();
        manifest.sessions[0].sha256.clear();
        let issues = validate(&manifest);
        assert!(
            issues
                .iter()
                .any(|issue| issue.path == "$.sessions[0].sha256")
        );
    }

    #[test]
    fn missing_keyed_session_is_reported() {
        let mut expected = synthetic();
        let second = {
            let mut value = expected.sessions[0].clone();
            value.session_id = "session_0002".to_owned();
            value.source_id_sha256 =
                "4dc99fe74efd88c53459e93de1e65a2912047740db3edabb79e17f2efc195145".to_owned();
            refresh_aggregate_digests(&mut value);
            value
        };
        expected.sessions.push(second);
        assert_eq!(
            expected.sessions[0].time.start_utc,
            expected.sessions[1].time.start_utc
        );
        assert_eq!(validate(&expected), Vec::new());
        let actual = synthetic_subject();
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert_eq!(report.differences[0].kind, DifferenceKind::Missing);
    }

    #[test]
    fn missing_keyed_channel_is_reported() {
        let mut expected = synthetic();
        let second = {
            let mut value = expected.sessions[0].waveforms.channels[0].clone();
            value.channel_id = "pap.series.mask_pressure_high_rate".to_owned();
            value.unit = "cmH2O".to_owned();
            value
        };
        expected.sessions[0].waveforms.channels.push(second);
        expected.sessions[0].waveforms.channel_count = 2;
        refresh_aggregate_digests(&mut expected.sessions[0]);
        let actual = synthetic_subject();
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert!(report.differences.iter().any(|difference| {
            difference.kind == DifferenceKind::Missing
                && difference.path.contains("channels[expected:1]")
        }));
    }

    #[test]
    fn invalid_tolerance_cannot_turn_mismatches_into_passes() {
        let expected = synthetic();
        let mut actual = synthetic_subject();
        actual.sessions[0].waveforms.channels[0].head_samples[0] = 999.0;
        let mut tolerances = DEFAULT_FLOAT_TOLERANCES;
        tolerances.waveform_preview_sample_abs = f64::NAN;
        let error = compare(&expected, &actual, tolerances).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid float tolerance profile")
        );
    }

    #[test]
    fn timestamps_are_structurally_validated() {
        let mut manifest = synthetic();
        manifest.sessions[0].time.start_utc = "not-a-dateTnopeZ".to_owned();
        manifest.sessions[0].time.end_local = "2025-02-30T05:30:00.000".to_owned();
        let issues = validate(&manifest);
        assert!(issues.iter().any(|issue| issue.path.ends_with("start_utc")));
        assert!(issues.iter().any(|issue| issue.path.ends_with("end_local")));
    }

    #[test]
    fn timestamps_must_be_canonical_ordered_and_consistent() {
        let mut manifest = synthetic();
        manifest.sessions[0].time.start_utc = "2025-01-01T15:00:00.0Z".to_owned();
        let issues = validate(&manifest);
        assert!(issues.iter().any(|issue| issue.path.ends_with("start_utc")));

        let mut manifest = synthetic();
        manifest.sessions[0].time.end_utc = "2025-01-01T14:00:00.000Z".to_owned();
        let issues = validate(&manifest);
        assert!(
            issues.iter().any(|issue| {
                issue.path.ends_with("end_utc") && issue.message.contains("later")
            })
        );

        let mut manifest = synthetic();
        manifest.sessions[0].time.start_utc_offset_seconds = Some(25_260);
        let issues = validate(&manifest);
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("inconsistent"))
        );
    }

    #[test]
    fn absent_source_offsets_are_preserved_without_inventing_a_timezone() {
        let mut manifest = synthetic();
        manifest.sessions[0].time.start_utc_offset_seconds = None;
        manifest.sessions[0].time.end_utc_offset_seconds = None;
        assert_eq!(validate(&manifest), Vec::new());
    }

    #[test]
    fn decimals_use_one_canonical_grammar() {
        for invalid in ["-03276.8", ".5", "1.0", "-0", "123456789"] {
            let mut manifest = synthetic();
            manifest.sessions[0].waveforms.channels[0]
                .encoding
                .physical_min_decimal = invalid.to_owned();
            assert!(validate(&manifest).iter().any(|issue| {
                issue.path.ends_with("physical_min_decimal")
                    && issue.message.contains("canonical EDF decimal")
            }));
        }
    }

    #[test]
    fn overlapping_waveform_previews_must_agree() {
        let mut manifest = synthetic();
        let waveform = &mut manifest.sessions[0].waveforms.channels[0];
        waveform.sample_count = 16;
        waveform.head_samples.truncate(16);
        waveform.tail_samples = waveform.head_samples.clone();
        waveform.tail_samples[5] += 1.0;
        assert!(validate(&manifest).iter().any(|issue| {
            issue.path.contains("tail_samples") && issue.message.contains("overlapping head sample")
        }));
    }

    #[test]
    fn events_stay_inside_session() {
        let mut manifest = synthetic();
        manifest.sessions[0].events.channels[0].items[1].offset_milliseconds = 30_000_000;
        assert!(
            validate(&manifest)
                .iter()
                .any(|issue| { issue.message.contains("beyond the session end") })
        );
    }

    #[test]
    fn event_time_and_source_identity_compare_exactly() {
        let expected = synthetic();
        let mut actual = synthetic_subject();
        let actual_events = &mut actual.sessions[0].events.channels[0].items;
        actual_events[0].offset_milliseconds += 1;
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert!(report.differences.iter().any(|difference| {
            difference.path.ends_with("offset_milliseconds")
                && difference.kind == DifferenceKind::ExactMismatch
        }));

        let mut actual = synthetic_subject();
        actual.sessions[0].events.channels[0].items[0].source_id_sha256 =
            "c2b3c7c3794199da42bccc267fd75ba2dfd365a6debec0d01d062dd6afe91bc7".to_owned();
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert!(
            report
                .differences
                .iter()
                .any(|difference| difference.path.ends_with("source_id_sha256"))
        );
    }

    #[test]
    fn channel_identifiers_follow_the_ascii_schema_contract() {
        let mut manifest = synthetic();
        manifest.sessions[0].events.channels[0].channel_id = "Bad/ID".to_owned();
        manifest.sessions[0].waveforms.channels[0].channel_id = "Flow/Rate".to_owned();
        let issues = validate(&manifest);
        assert_eq!(
            issues
                .iter()
                .filter(|issue| {
                    issue.path.ends_with("channel_id")
                        && issue.message.contains("must use only lowercase ASCII")
                })
                .count(),
            2
        );
    }

    #[test]
    fn dst_fall_back_can_reverse_local_wall_clock_order() {
        let mut manifest = synthetic();
        let session = &mut manifest.sessions[0];
        session.time.start_utc = "2025-11-02T05:45:00.000Z".to_owned();
        session.time.end_utc = "2025-11-02T06:15:00.000Z".to_owned();
        session.time.start_local = "2025-11-02T01:45:00.000".to_owned();
        session.time.end_local = "2025-11-02T01:15:00.000".to_owned();
        session.time.start_utc_offset_seconds = Some(-14_400);
        session.time.end_utc_offset_seconds = Some(-18_000);
        session.slices.items[0].end_offset_milliseconds = 1_800_000;
        session.summary.usage_milliseconds = 1_800_000;
        session.events.channels[0].items[0].offset_milliseconds = 100_000;
        session.events.channels[0].items[1].offset_milliseconds = 500_000;
        refresh_aggregate_digests(session);
        assert_eq!(validate(&manifest), Vec::new());
    }

    #[test]
    fn aggregate_digests_match_the_public_vector() {
        let vector: AggregateDigestVector = serde_json::from_str(include_str!(
            "../tests/fixtures/aggregate-digest-vector.json"
        ))
        .unwrap();
        assert_eq!(
            slice_collection_sha256(&vector.slices),
            vector.slice_collection_sha256
        );
        assert_eq!(
            event_collection_sha256(
                vector
                    .event_channels
                    .iter()
                    .map(|pair| (pair[0].as_str(), pair[1].as_str()))
            ),
            vector.event_collection_sha256
        );
        assert_eq!(
            waveform_collection_sha256(vector.waveform_channels.iter().map(|triple| (
                triple[0].as_str(),
                triple[1].as_str(),
                triple[2].as_str()
            ))),
            vector.waveform_collection_sha256
        );
        assert_eq!(
            session_aggregate_sha256(
                &vector.session.source_id_sha256,
                &vector.session.source_sha256,
                &vector.session.slices_sha256,
                &vector.session.summary_source_sha256,
                &vector.session.settings_sha256,
                &vector.session.events_sha256,
                &vector.session.waveforms_sha256
            ),
            vector.session_sha256
        );
    }

    #[test]
    fn source_digests_match_the_public_vector() {
        let vector: SourceDigestVector =
            serde_json::from_str(include_str!("../tests/fixtures/source-digest-vector.json"))
                .unwrap();
        let record_digest =
            record_stream_sha256(vector.records_utf8.iter().map(|record| record.as_bytes()))
                .unwrap();
        assert_eq!(record_digest, vector.record_stream_sha256);
        let tree_digest = source_tree_sha256(
            vector
                .tree
                .iter()
                .map(|entry| (entry.path.as_str(), entry.contents_utf8.as_bytes())),
        )
        .unwrap();
        assert_eq!(tree_digest, vector.source_tree_sha256);

        assert_eq!(
            source_tree_sha256([
                ("STR.edf", b"one".as_slice()),
                ("Identification.json", b"two".as_slice())
            ]),
            Err(SourceDigestError::UnsortedOrDuplicatePath { index: 1 })
        );
        assert_eq!(
            source_tree_sha256([("../private/card.edf", b"secret".as_slice())]),
            Err(SourceDigestError::NonCanonicalPath { index: 0 })
        );
    }

    #[test]
    fn exact_integer_normalization_never_rounds_near_json_safe_limit() {
        for (raw, expected) in [
            ("3.0", 3),
            ("3e0", 3),
            ("3.00e+1", 30),
            ("30e-1", 3),
            ("0.03e2", 3),
            ("-0.0", 0),
            ("9007199254740991.0", 9_007_199_254_740_991),
        ] {
            let value: serde_json::Value = serde_json::from_str(raw).unwrap();
            assert_eq!(
                exact_json_safe_integer(value.as_number().unwrap()),
                Some(expected),
                "{raw}"
            );
        }
        for raw in [
            "9007199254740991.1",
            "9007199254740992.0",
            "-9007199254740992.0",
            "1e16",
            "3.1",
            "1e-1",
            "1e999999999999999999999999999999999999999999",
        ] {
            let value: serde_json::Value = serde_json::from_str(raw).unwrap();
            assert_eq!(
                exact_json_safe_integer(value.as_number().unwrap()),
                None,
                "{raw}"
            );
        }
    }

    #[test]
    fn integral_json_spellings_load_but_fractional_safe_limit_does_not() {
        for spelling in ["3.0", "3e0"] {
            let raw = include_str!("../tests/fixtures/synthetic-oscar.json").replacen(
                "\"count\": 3",
                &format!("\"count\": {spelling}"),
                1,
            );
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), raw).unwrap();
            assert!(load_manifest(file.path()).is_ok(), "{spelling}");
        }

        let raw = include_str!("../tests/fixtures/synthetic-oscar.json").replacen(
            "\"count\": 3",
            "\"count\": 9007199254740991.1",
            1,
        );
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), raw).unwrap();
        assert!(load_manifest(file.path()).is_err());
    }

    #[test]
    fn explicit_empty_collections_are_valid_and_missing_expected_members_still_fail() {
        let mut manifest = synthetic();
        let session = &mut manifest.sessions[0];
        session.settings.count = 0;
        session.settings.items.clear();
        session.summary.metric_count = 0;
        session.summary.metrics.clear();
        session.events.channel_count = 0;
        session.events.channels.clear();
        session.waveforms.channel_count = 0;
        session.waveforms.channels.clear();
        refresh_aggregate_digests(session);
        assert_eq!(validate(&manifest), Vec::new());

        let report = compare(
            &synthetic(),
            &{
                let mut subject = manifest;
                subject.producer = synthetic_subject().producer;
                subject
            },
            DEFAULT_FLOAT_TOLERANCES,
        )
        .unwrap();
        assert!(!report.compatible);
        assert!(
            report
                .differences
                .iter()
                .any(|difference| difference.kind == DifferenceKind::Missing)
        );
    }

    #[test]
    fn nullable_event_payloads_and_empty_event_channels_are_valid() {
        let mut manifest = synthetic();
        {
            let channel = &mut manifest.sessions[0].events.channels[0];
            channel.items[0].duration_milliseconds = None;
            channel.items[0].value = NullableNumber(None);
        }
        assert_eq!(validate(&manifest), Vec::new());

        manifest.sessions[0].events.channels[0].items.clear();
        manifest.sessions[0].events.channels[0].count = 0;
        assert_eq!(validate(&manifest), Vec::new());
    }

    #[test]
    fn slices_summaries_and_waveform_segments_have_comparison_effect() {
        let expected = synthetic();

        let mut actual = synthetic_subject();
        actual.sessions[0].slices.items[0].source_id_sha256 =
            "33bb05ff493a095a575870b61c01b691c52f7f32d89b9637c104aaf64d4849a1".to_owned();
        refresh_aggregate_digests(&mut actual.sessions[0]);
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert!(
            report
                .differences
                .iter()
                .any(|difference| difference.path.contains("slices.items"))
        );

        let mut actual = synthetic_subject();
        actual.sessions[0].summary.metrics[0].value = NullableNumber(Some(
            expected.sessions[0].summary.metrics[0].value.0.unwrap()
                + DEFAULT_FLOAT_TOLERANCES.summary_metric_abs * 2.0,
        ));
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert!(
            report
                .differences
                .iter()
                .any(|difference| difference.path.contains("summary.metrics"))
        );

        let mut actual = synthetic_subject();
        actual.sessions[0].waveforms.channels[0].segments[1].start_offset_milliseconds += 1;
        let report = compare(&expected, &actual, DEFAULT_FLOAT_TOLERANCES).unwrap();
        assert!(!report.compatible);
        assert!(
            report
                .differences
                .iter()
                .any(|difference| difference.path.contains("segments[1]"))
        );
    }

    #[test]
    fn real_card_manifests_require_verified_adapter_attestation() {
        let mut manifest = synthetic();
        manifest.fixture.synthetic = false;
        assert!(validate(&manifest).iter().any(|issue| {
            issue.path == "$.producer.adapter_attestation"
                && issue.message.contains("real-card comparisons require")
        }));
        manifest.producer.adapter_attestation = AdapterAttestationKind::VerifiedCleanTree;
        assert!(
            !validate(&manifest)
                .iter()
                .any(|issue| issue.path == "$.producer.adapter_attestation")
        );
    }

    #[test]
    fn waveform_segments_reject_overlap_and_incomplete_coverage() {
        let mut manifest = synthetic();
        manifest.sessions[0].waveforms.channels[0].segments[1].start_offset_milliseconds = 6_599;
        assert!(
            validate(&manifest)
                .iter()
                .any(|issue| issue.message.contains("must not overlap"))
        );

        let mut manifest = synthetic();
        manifest.sessions[0].waveforms.channels[0].segments[1].sample_count = 39;
        assert!(
            validate(&manifest)
                .iter()
                .any(|issue| issue.message.contains("exactly cover"))
        );
    }
}
