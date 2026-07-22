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
    InProgress,
    Completed,
    Failed,
}

impl ImportStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
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
    pub status: ImportStatus,
    pub started_at_ms: i64,
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
    /// Fingerprint of the logical import, scoped to a profile.
    pub import_key: &'a str,
    pub source_uri: &'a str,
    pub loader_name: &'a str,
    pub started_at_ms: i64,
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
