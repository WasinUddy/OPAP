#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: i64,
    pub display_name: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct NewProfile<'a> {
    pub display_name: &'a str,
    pub now_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    pub id: i64,
    pub profile_id: i64,
    pub source_key: String,
    pub device_type: String,
    pub manufacturer: String,
    pub model: String,
    pub model_number: String,
    pub serial_number: String,
    pub first_seen_at_ms: i64,
    pub last_seen_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct NewMachine<'a> {
    pub profile_id: i64,
    /// Stable identity supplied by the importer, scoped to a profile.
    pub source_key: &'a str,
    pub device_type: &'a str,
    pub manufacturer: &'a str,
    pub model: &'a str,
    pub model_number: &'a str,
    pub serial_number: &'a str,
    pub seen_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: i64,
    pub machine_id: i64,
    pub source_key: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub timezone_offset_minutes: Option<i32>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct NewSession<'a> {
    pub machine_id: i64,
    /// Stable session identity supplied by the importer, scoped to a machine.
    pub source_key: &'a str,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub timezone_offset_minutes: Option<i32>,
    pub now_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: i64,
    pub session_id: i64,
    pub source_key: String,
    pub channel_key: String,
    pub event_type: String,
    pub starts_at_ms: i64,
    pub duration_ms: Option<i64>,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct NewEvent<'a> {
    pub session_id: i64,
    /// Stable event identity supplied by the importer, scoped to a session.
    pub source_key: &'a str,
    pub channel_key: &'a str,
    pub event_type: &'a str,
    pub starts_at_ms: i64,
    pub duration_ms: Option<i64>,
    pub value: Option<f64>,
    pub unit: Option<&'a str>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaveformMetadata {
    pub id: i64,
    pub session_id: i64,
    pub source_key: String,
    pub channel_key: String,
    pub unit: Option<String>,
    pub started_at_ms: i64,
    pub sample_interval_us: i64,
    pub sample_count: i64,
    pub encoding: String,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct NewWaveformMetadata<'a> {
    pub session_id: i64,
    /// Stable waveform identity supplied by the importer, scoped to a session.
    pub source_key: &'a str,
    pub channel_key: &'a str,
    pub unit: Option<&'a str>,
    pub started_at_ms: i64,
    pub sample_interval_us: i64,
    pub sample_count: i64,
    /// Describes the binary chunk payload, for example `f32-le`.
    pub encoding: &'a str,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaveformChunk {
    pub waveform_id: i64,
    pub chunk_index: i64,
    pub start_sample: i64,
    pub sample_count: i64,
    pub payload: Vec<u8>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct NewWaveformChunk<'a> {
    pub waveform_id: i64,
    pub chunk_index: i64,
    pub start_sample: i64,
    pub sample_count: i64,
    pub payload: &'a [u8],
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStatus {
    Blocked,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ImportStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "blocked" => Some(Self::Blocked),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// States in which a new or retried import job may honestly be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialImportStatus {
    Blocked,
    Running,
}

impl InitialImportStatus {
    pub(crate) const fn status(self) -> ImportStatus {
        match self {
            Self::Blocked => ImportStatus::Blocked,
            Self::Running => ImportStatus::Running,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportHistory {
    pub id: i64,
    pub profile_id: i64,
    pub machine_id: Option<i64>,
    pub import_key: String,
    pub source_uri: String,
    pub loader_name: String,
    pub attempt: i64,
    pub retry_of_id: Option<i64>,
    pub status: ImportStatus,
    pub state_message: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
    pub sessions_created: i64,
    pub sessions_updated: i64,
    pub events_written: i64,
    pub waveform_chunks_written: i64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct NewImport<'a> {
    pub profile_id: i64,
    pub machine_id: Option<i64>,
    /// Opaque service-generated request id, scoped to a profile.
    pub import_key: &'a str,
    pub source_uri: &'a str,
    pub loader_name: &'a str,
    pub initial_status: InitialImportStatus,
    pub state_message: Option<&'a str>,
    pub created_at_ms: i64,
}

pub const SOURCE_ID_PREFIX: &str = "opap-source:";
pub const LEGACY_SOURCE_ID_PREFIX: &str = "opap-source:legacy-";
pub const REQUEST_ID_PREFIX: &str = "opap-request:";
pub const LEGACY_REQUEST_ID_PREFIX: &str = "opap-request:legacy-";

fn has_lower_hex_suffix(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

/// Returns true only for IDs produced by the native source capability registry:
/// `opap-source:` followed by exactly 32 lowercase hexadecimal characters.
pub fn is_canonical_source_id(value: &str) -> bool {
    has_lower_hex_suffix(value, SOURCE_ID_PREFIX)
}

/// Controlled placeholders used only when a pre-privacy-schema record contained
/// a path. The positive decimal suffix is the stable import-history row id.
pub fn is_legacy_source_id(value: &str) -> bool {
    value
        .strip_prefix(LEGACY_SOURCE_ID_PREFIX)
        .is_some_and(|suffix| {
            suffix.len() <= 19
                && suffix
                    .as_bytes()
                    .split_first()
                    .is_some_and(|(first, rest)| {
                        matches!(first, b'1'..=b'9') && rest.iter().all(u8::is_ascii_digit)
                    })
        })
}

pub fn is_persistable_source_id(value: &str) -> bool {
    is_canonical_source_id(value) || is_legacy_source_id(value)
}

/// Returns true only for opaque request IDs generated by the service:
/// `opap-request:` followed by exactly 32 lowercase hexadecimal characters.
pub fn is_canonical_request_id(value: &str) -> bool {
    has_lower_hex_suffix(value, REQUEST_ID_PREFIX)
}

/// Controlled placeholders written by the v7 privacy migration. The decimal
/// suffix is a positive SQLite row id and therefore must fit in an `i64`.
pub fn is_legacy_request_id(value: &str) -> bool {
    value
        .strip_prefix(LEGACY_REQUEST_ID_PREFIX)
        .and_then(|suffix| {
            let (first, rest) = suffix.as_bytes().split_first()?;
            (matches!(first, b'1'..=b'9') && rest.iter().all(u8::is_ascii_digit))
                .then(|| suffix.parse::<i64>().ok())
                .flatten()
        })
        .is_some_and(|row_id| row_id > 0)
}

pub fn is_persistable_import_key(value: &str) -> bool {
    is_canonical_request_id(value) || is_legacy_request_id(value)
}

#[derive(Debug, Clone, Copy)]
pub struct RetryImport<'a> {
    pub initial_status: InitialImportStatus,
    pub state_message: Option<&'a str>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub sessions_created: i64,
    pub sessions_updated: i64,
    pub events_written: i64,
    pub waveform_chunks_written: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginImport {
    pub history: ImportHistory,
    pub inserted: bool,
}

/// Typed state-machine commands. Repository methods enforce each command's
/// allowed source states in the same SQL statement that persists the change.
#[derive(Debug, Clone, Copy)]
pub enum ImportTransition<'a> {
    Block { at_ms: i64, reason: &'a str },
    Start { at_ms: i64 },
    Complete { at_ms: i64, counts: ImportCounts },
    Fail { at_ms: i64, error: &'a str },
    Cancel { at_ms: i64, reason: Option<&'a str> },
}

/// An event in an authoritative replacement of one session's derived data.
#[derive(Debug, Clone, Copy)]
pub struct SessionEventInput<'a> {
    pub source_key: &'a str,
    pub channel_key: &'a str,
    pub event_type: &'a str,
    pub starts_at_ms: i64,
    pub duration_ms: Option<i64>,
    pub value: Option<f64>,
    pub unit: Option<&'a str>,
    pub created_at_ms: i64,
}

/// A chunk nested under a waveform during an authoritative session replacement.
#[derive(Debug, Clone, Copy)]
pub struct SessionWaveformChunkInput<'a> {
    pub chunk_index: i64,
    pub start_sample: i64,
    pub sample_count: i64,
    pub payload: &'a [u8],
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
}

/// Waveform metadata and its complete ordered chunk set.
#[derive(Debug, Clone, Copy)]
pub struct SessionWaveformInput<'a> {
    pub source_key: &'a str,
    pub channel_key: &'a str,
    pub unit: Option<&'a str>,
    pub started_at_ms: i64,
    pub sample_interval_us: i64,
    pub sample_count: i64,
    pub encoding: &'a str,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub created_at_ms: i64,
    pub chunks: &'a [SessionWaveformChunkInput<'a>],
}

/// The complete event and waveform state produced for one session import.
/// Records omitted from either slice are pruned when replacement commits.
#[derive(Debug, Clone, Copy)]
pub struct SessionDataReplacement<'a> {
    pub events: &'a [SessionEventInput<'a>],
    pub waveforms: &'a [SessionWaveformInput<'a>],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionReplacementStats {
    pub events_written: usize,
    pub events_pruned: usize,
    pub waveforms_written: usize,
    pub waveforms_pruned: usize,
    pub waveform_chunks_written: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionReplacementResult {
    pub session: Session,
    pub stats: SessionReplacementStats,
}

/// Completeness of the source data represented by a persisted session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDataKind {
    Detailed,
    SummaryOnly,
    Partial,
}

impl SessionDataKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Detailed => "detailed",
            Self::SummaryOnly => "summary_only",
            Self::Partial => "partial",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "detailed" => Some(Self::Detailed),
            "summary_only" => Some(Self::SummaryOnly),
            "partial" => Some(Self::Partial),
            _ => None,
        }
    }
}

/// Source-clock and importer provenance retained for a complete session
/// snapshot. Digests are opaque lowercase SHA-256 values; raw paths, serial
/// numbers, and patient identifiers must never be used in their place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionProvenance {
    pub session_id: i64,
    pub therapy_day: String,
    pub start_local_wall: String,
    pub end_local_wall: String,
    pub start_utc_offset_seconds: Option<i32>,
    pub end_utc_offset_seconds: Option<i32>,
    pub start_clock_correction_ms: i64,
    pub end_clock_correction_ms: i64,
    pub data_kind: SessionDataKind,
    pub importer_name: String,
    pub importer_schema: String,
    pub id_algorithm: String,
    pub source_digest: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SessionProvenanceInput<'a> {
    /// Manufacturer/source therapy-day bucket in `YYYY-MM-DD` form.
    pub therapy_day: &'a str,
    /// Source-local wall timestamps in exact `YYYY-MM-DDTHH:MM:SS.mmm` form.
    pub start_local_wall: &'a str,
    pub end_local_wall: &'a str,
    /// Signed seconds by which source-local time is ahead of UTC.
    pub start_utc_offset_seconds: Option<i32>,
    pub end_utc_offset_seconds: Option<i32>,
    /// Signed corrections added to the source clock during normalization.
    pub start_clock_correction_ms: i64,
    pub end_clock_correction_ms: i64,
    pub data_kind: SessionDataKind,
    pub importer_name: &'a str,
    pub importer_schema: &'a str,
    pub id_algorithm: &'a str,
    pub source_digest: &'a str,
    pub content_digest: &'a str,
}

/// Therapy/equipment state represented by a stored session slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSliceState {
    MaskOn,
    MaskOff,
    EquipmentOff,
}

impl SessionSliceState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::MaskOn => "mask_on",
            Self::MaskOff => "mask_off",
            Self::EquipmentOff => "equipment_off",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "mask_on" => Some(Self::MaskOn),
            "mask_off" => Some(Self::MaskOff),
            "equipment_off" => Some(Self::EquipmentOff),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSlice {
    pub session_id: i64,
    pub sequence: i64,
    pub source_key: String,
    pub state: SessionSliceState,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct SessionSliceInput<'a> {
    /// Zero-based source order. Snapshot replacement requires contiguous values.
    pub sequence: i64,
    /// Stable opaque importer-derived interval identity.
    pub source_key: &'a str,
    pub state: SessionSliceState,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SummaryMetric {
    pub session_id: i64,
    pub key: String,
    pub value: f64,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct SummaryMetricInput<'a> {
    pub key: &'a str,
    pub value: f64,
    pub unit: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub session_id: i64,
    pub usage_ms: i64,
    pub metrics: Vec<SummaryMetric>,
}

#[derive(Debug, Clone, Copy)]
pub struct SessionSummaryInput<'a> {
    pub usage_ms: i64,
    pub metrics: &'a [SummaryMetricInput<'a>],
}

/// Typed value recovered from a `session_settings` row.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionSettingValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Boolean(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSetting {
    pub session_id: i64,
    pub key: String,
    pub value: SessionSettingValue,
    pub unit: Option<String>,
    pub origin: String,
}

/// Relational setting input. Exactly one value field must be populated. Keeping
/// the representation explicit lets storage reject malformed adapter output
/// before opening a transaction.
#[derive(Debug, Clone, Copy)]
pub struct SessionSettingInput<'a> {
    pub key: &'a str,
    pub integer_value: Option<i64>,
    pub real_value: Option<f64>,
    pub text_value: Option<&'a str>,
    pub boolean_value: Option<bool>,
    pub unit: Option<&'a str>,
    pub origin: &'a str,
}

/// New v8 children that accompany the authoritative event/waveform data for a
/// session. All slices, metrics, and settings omitted here are pruned.
#[derive(Debug, Clone, Copy)]
pub struct SessionSnapshotReplacement<'a> {
    pub data: SessionDataReplacement<'a>,
    pub provenance: SessionProvenanceInput<'a>,
    pub slices: &'a [SessionSliceInput<'a>],
    pub summary: SessionSummaryInput<'a>,
    pub settings: &'a [SessionSettingInput<'a>],
}

/// Complete v8 child state read from storage.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSnapshot {
    pub provenance: SessionProvenance,
    pub slices: Vec<SessionSlice>,
    pub summary: SessionSummary,
    pub settings: Vec<SessionSetting>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionSnapshotReplacementStats {
    pub session_data: SessionReplacementStats,
    pub slices_written: usize,
    pub slices_pruned: usize,
    pub summary_metrics_written: usize,
    pub summary_metrics_pruned: usize,
    pub settings_written: usize,
    pub settings_pruned: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSnapshotReplacementResult {
    pub session: Session,
    pub stats: SessionSnapshotReplacementStats,
}
