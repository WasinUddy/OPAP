// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2026 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from OSCAR concepts:
// https://gitlab.com/CrimsonNape/OSCAR-code
// Upstream commit: 64c5e90a26f91fb15868bcfcccde0c1e1522ac86
// Relevant upstream files: oscar/SleepLib/machine_common.h, oscar/SleepLib/machine.h,
// oscar/SleepLib/session.h, oscar/SleepLib/event.h
// Modified: 2026-07-23

//! Serializable domain types shared by importers and application frontends.
//!
//! These types deliberately avoid native filesystem and UI concepts. Their
//! serialized representation is the contract that a desktop host, command-line
//! client, or future WebAssembly wrapper consumes.

use serde::{Deserialize, Serialize};

/// Current version of the serialized import report contract.
pub const IMPORT_SCHEMA_VERSION: u16 = 4;

/// A timestamp expressed as milliseconds since the Unix epoch in UTC.
pub type UnixMillis = i64;

/// CPAP machine identity reported by the device.
///
/// Fields remain strings because manufacturers commonly use leading zeroes and
/// non-numeric characters in product codes and serial numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineInfo {
    /// Device manufacturer, for example `ResMed`.
    pub brand: String,
    /// Human-readable display product name, normalized where the source format
    /// requires it.
    pub model: String,
    /// Manufacturer product-name value before OPAP display normalization.
    ///
    /// ResMed JSON preserves the decoded `ProductName` string verbatim without
    /// trimming. Legacy TGT preserves the trimmed `PNA` value, including its
    /// source underscores/parentheses.
    /// Empty means the source supplied no product name. `serde(default)` keeps
    /// schema-v2 records readable while schema v3 writes the explicit field.
    #[serde(default)]
    pub source_model: String,
    /// Manufacturer product code.
    pub model_number: String,
    /// Manufacturer serial number.
    pub serial: String,
    /// Product family, for example `AirSense 11`.
    pub series: String,
}

impl Default for MachineInfo {
    fn default() -> Self {
        Self {
            brand: "ResMed".to_owned(),
            model: String::new(),
            source_model: String::new(),
            model_number: String::new(),
            serial: String::new(),
            series: String::new(),
        }
    }
}

/// A detected device together with the importer that recognized it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Stable importer identifier, such as `resmed`.
    pub importer_id: String,
    /// Identity read from the device media.
    pub machine: MachineInfo,
}

/// Semantic role of a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// A regularly sampled signal such as flow rate or mask pressure.
    Waveform,
    /// Discrete annotations such as apnea flags.
    Event,
    /// A numeric value summarized over a session.
    Summary,
}

/// Metadata that describes a channel independently of its samples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelMetadata {
    /// Stable importer-defined channel identifier.
    pub id: String,
    /// Human-readable channel name.
    pub label: String,
    /// Unit symbol, omitted for unitless channels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// How values in this channel should be interpreted.
    pub kind: ChannelKind,
}

/// Regularly sampled values for one channel in one session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaveformSeries {
    /// Opaque, importer-derived identity of the source series.
    ///
    /// This value must be stable for idempotent re-imports and must not contain
    /// a filesystem path, patient identifier, or machine serial number. An empty
    /// value denotes a record produced before schema v4.
    #[serde(default)]
    pub source_key: String,
    /// Identifier of the corresponding [`ChannelMetadata`].
    pub channel_id: String,
    /// UTC time of the first sample.
    pub start_time_unix_ms: UnixMillis,
    /// Milliseconds between consecutive samples.
    pub sample_interval_ms: f64,
    /// Samples in chronological order, in the channel's declared unit.
    pub samples: Vec<f32>,
    /// Original EDF calibration and record cadence, when the source was EDF.
    ///
    /// Importers must validate that every bound and duration is finite, that
    /// digital bounds differ, and that the record cadence is non-zero before
    /// using this metadata to interpret samples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_encoding: Option<EdfSourceEncoding>,
}

/// Calibration and cadence copied from one decoded EDF signal.
///
/// This metadata preserves enough source context to audit the normalized flat
/// samples without requiring consumers to reinterpret untrusted EDF headers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdfSourceEncoding {
    /// Smallest digital value declared by the EDF signal.
    pub digital_minimum: i32,
    /// Largest digital value declared by the EDF signal.
    pub digital_maximum: i32,
    /// Physical value corresponding to the declared digital minimum.
    pub physical_minimum: f64,
    /// Physical value corresponding to the declared digital maximum.
    pub physical_maximum: f64,
    /// Samples contributed by this signal to each EDF data record.
    pub samples_per_record: u32,
    /// Duration of one EDF data record in seconds.
    pub record_duration_seconds: f64,
}

/// One discrete event in an [`EventSeries`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Opaque, importer-derived identity of the source event.
    ///
    /// This must be stable within the session and must not expose source paths
    /// or device identifiers. Empty denotes a record produced before schema v4.
    #[serde(default)]
    pub source_key: String,
    /// UTC time at which the event starts.
    pub start_time_unix_ms: UnixMillis,
    /// Event duration.
    ///
    /// `None` means the source did not report a duration. `Some(0)` is distinct
    /// and represents a source-reported instantaneous event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Optional event magnitude or manufacturer-provided score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// Chronologically ordered events belonging to one event channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSeries {
    /// Identifier of the corresponding [`ChannelMetadata`].
    pub channel_id: String,
    /// Imported events in chronological order.
    pub events: Vec<Event>,
}

/// Typed value used for device and therapy settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SettingValue {
    /// Text or an importer-defined enumeration label.
    Text(String),
    /// Signed whole number.
    Integer(i64),
    /// Floating-point number.
    Decimal(f64),
    /// On/off value.
    Boolean(bool),
}

/// Evidence for how a setting value entered the normalized session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ValueOrigin {
    /// The device explicitly reported the value.
    DeviceReported,
    /// The importer calculated the value with a named, versionable algorithm.
    Derived {
        /// Stable algorithm identifier. Importers must not use a display label
        /// or free-form explanation as this identifier.
        algorithm: String,
    },
    /// The value was inferred without a direct device field.
    Inferred,
}

impl Default for ValueOrigin {
    fn default() -> Self {
        Self::Inferred
    }
}

/// One machine or therapy setting active during a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setting {
    /// Stable importer-defined setting identifier.
    pub key: String,
    /// Human-readable setting name.
    pub label: String,
    /// Unit symbol for numeric settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Typed setting value.
    pub value: SettingValue,
    /// Provenance of the normalized value.
    ///
    /// Schema-v3 settings deserialize as [`ValueOrigin::Inferred`] so legacy
    /// values are never retroactively presented as device-reported facts.
    #[serde(default)]
    pub origin: ValueOrigin,
}

/// One numeric value summarized over a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryMetric {
    /// Stable importer-defined metric identifier.
    pub key: String,
    /// Human-readable metric name.
    pub label: String,
    /// Metric value in the declared unit.
    pub value: f64,
    /// Unit symbol, omitted for unitless metrics such as AHI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// Values calculated or reported for a complete therapy session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Therapy usage represented by the session.
    pub usage_ms: u64,
    /// Importer-defined summary metrics.
    pub metrics: Vec<SummaryMetric>,
}

/// Device-local calendar time with no implied UTC offset or timezone.
///
/// Importers must validate calendar ranges before constructing normalized
/// timestamps: month 1–12, a day valid for its month and year, hour 0–23,
/// minute and second 0–59, and millisecond 0–999. This type deliberately does
/// not guess daylight-saving rules or interpret local fields as UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceLocalDateTime {
    /// Full calendar year.
    pub year: u16,
    /// Calendar month, 1 through 12.
    pub month: u8,
    /// Calendar day, validated against the month and year.
    pub day: u8,
    /// Hour, 0 through 23.
    pub hour: u8,
    /// Minute, 0 through 59.
    pub minute: u8,
    /// Second, 0 through 59.
    pub second: u8,
    /// Millisecond, 0 through 999.
    pub millisecond: u16,
}

/// A session boundary with both normalized and source-clock provenance.
///
/// Keeping the device-local wall time permits timezone rules and clock
/// corrections to be reapplied without reparsing the source card. When an
/// offset is present it is the signed number of seconds local time is ahead of
/// UTC. `device_clock_correction_ms` is the signed correction added to the raw
/// device time before normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTimestamp {
    /// Timestamp normalized to UTC after applying the recorded offset and
    /// device correction.
    pub normalized_utc_unix_ms: UnixMillis,
    /// Device-local wall time before offset or clock correction, serialized as
    /// an ISO 8601 local date-time without a timezone suffix.
    pub device_local_wall_time: String,
    /// Structured device-local fields, avoiding reparsing the legacy string.
    ///
    /// Schema-v3 records deserialize this as `None`. Importers writing schema v4
    /// should populate it only after validating the calendar fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_local: Option<DeviceLocalDateTime>,
    /// Signed seconds by which device-local time was ahead of UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_utc_offset_seconds: Option<i32>,
    /// Signed correction added to the device clock before UTC normalization.
    pub device_clock_correction_ms: i64,
    /// Basis used to select the UTC offset, when known.
    ///
    /// Examples include a versioned timezone database rule or an explicitly
    /// supplied fixed offset. This is provenance, not an instruction to
    /// renormalize the timestamp. Empty legacy records deserialize as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone_basis: Option<String>,
}

/// Completeness of the data carried by a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDataKind {
    /// Detailed events or sampled signals were decoded for the session.
    Detailed,
    /// Only device-reported summary and settings data were available.
    SummaryOnly,
    /// Some expected source data was absent, rejected, or not decoded.
    Partial,
}

impl Default for SessionDataKind {
    fn default() -> Self {
        Self::Partial
    }
}

/// Therapy/equipment state represented by a [`TherapySlice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TherapySliceState {
    /// The mask was on and therapy usage accrued.
    MaskOn,
    /// The mask was off while the source session remained open.
    MaskOff,
    /// The device was explicitly reported as off.
    EquipmentOff,
}

/// One source-reported state interval within a therapy session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TherapySlice {
    /// Opaque, importer-derived identity of the source interval.
    ///
    /// It must not contain a filesystem path, patient identifier, or serial.
    #[serde(default)]
    pub source_key: String,
    /// State active throughout this half-open interval.
    pub state: TherapySliceState,
    /// Inclusive UTC start in milliseconds.
    pub start_time_unix_ms: UnixMillis,
    /// Exclusive UTC end in milliseconds.
    ///
    /// Importers must reject reversed intervals and overlapping slices rather
    /// than silently repairing or double-counting therapy usage.
    pub end_time_unix_ms: UnixMillis,
}

/// A single contiguous CPAP therapy session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Stable importer-derived identifier used for idempotent re-imports.
    pub id: String,
    /// Opaque identity of the complete source session.
    ///
    /// It must be stable for equivalent source content and must not expose raw
    /// source paths or device identifiers. Empty denotes a schema-v3 record.
    #[serde(default)]
    pub source_key: String,
    /// Manufacturer therapy-day bucket, in `YYYY-MM-DD` form when known.
    ///
    /// Importers must preserve the source device's day boundary; consumers must
    /// not reconstruct this value from normalized UTC timestamps.
    #[serde(default)]
    pub therapy_day: String,
    /// Whether the session contains detailed, summary-only, or partial data.
    #[serde(default)]
    pub data_kind: SessionDataKind,
    /// Start boundary, inclusive, with device-clock provenance.
    pub start_time: SessionTimestamp,
    /// End boundary, exclusive, with device-clock provenance.
    pub end_time: SessionTimestamp,
    /// Ordered, non-overlapping therapy/equipment intervals.
    #[serde(default)]
    pub slices: Vec<TherapySlice>,
    /// Channel definitions referenced by series in this session.
    pub channels: Vec<ChannelMetadata>,
    /// Regularly sampled signals.
    pub waveforms: Vec<WaveformSeries>,
    /// Discrete event channels.
    pub event_series: Vec<EventSeries>,
    /// Therapy settings active during the session.
    pub settings: Vec<Setting>,
    /// Session-level aggregates.
    pub summary: SessionSummary,
}

/// Importance assigned to a non-fatal import diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    /// Context useful for explaining an import result.
    Info,
    /// Data was missing, malformed, or only partially imported.
    Warning,
}

/// A structured, non-fatal diagnostic emitted during discovery or import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportWarning {
    /// Stable machine-readable warning code.
    pub code: String,
    /// Diagnostic importance.
    pub severity: WarningSeverity,
    /// Human-readable explanation.
    pub message: String,
    /// Normalized source-relative path associated with the warning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
    /// Session associated with the warning, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Counters describing work performed by an importer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportStatistics {
    /// Files present in the source inventory.
    pub files_discovered: u64,
    /// Files whose contents were read.
    pub files_read: u64,
    /// Total bytes read from source files.
    pub bytes_read: u64,
    /// Sessions emitted by this import.
    pub sessions_imported: u64,
    /// Recognized sessions deliberately omitted by import options.
    pub sessions_skipped: u64,
}

/// Complete serializable output of a successful device import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportReport {
    /// Version of this serialized contract.
    pub schema_version: u16,
    /// Stable identifier of the importer that produced the report.
    pub importer_id: String,
    /// Detected device identity.
    pub device: DeviceInfo,
    /// Imported sessions.
    pub sessions: Vec<Session>,
    /// Non-fatal diagnostics.
    pub warnings: Vec<ImportWarning>,
    /// Import counters.
    pub statistics: ImportStatistics,
}

impl ImportReport {
    /// Creates an empty report for a detected device using the current schema.
    #[must_use]
    pub fn empty(device: DeviceInfo) -> Self {
        Self {
            schema_version: IMPORT_SCHEMA_VERSION,
            importer_id: device.importer_id.clone(),
            device,
            sessions: Vec::new(),
            warnings: Vec::new(),
            statistics: ImportStatistics::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn setting_value_has_an_explicit_stable_json_tag() {
        assert_eq!(
            serde_json::to_value(SettingValue::Decimal(8.5)).expect("serialize setting"),
            json!({"type": "decimal", "value": 8.5})
        );
    }

    #[test]
    fn report_round_trips_without_native_types() {
        let device = DeviceInfo {
            importer_id: "resmed".to_owned(),
            machine: MachineInfo {
                brand: "ResMed".to_owned(),
                model: "AirSense 11 AutoSet".to_owned(),
                source_model: "AirSense11 AutoSet".to_owned(),
                model_number: "39001".to_owned(),
                serial: "123".to_owned(),
                series: "AirSense 11".to_owned(),
            },
        };
        let mut report = ImportReport::empty(device);
        report.warnings.push(ImportWarning {
            code: "partial_data".to_owned(),
            severity: WarningSeverity::Warning,
            message: "A source file was incomplete".to_owned(),
            relative_path: Some("DATALOG/example.edf".to_owned()),
            session_id: None,
        });

        let json = serde_json::to_vec(&report).expect("serialize report");
        let decoded: ImportReport = serde_json::from_slice(&json).expect("deserialize report");

        assert_eq!(decoded, report);
        assert_eq!(decoded.schema_version, IMPORT_SCHEMA_VERSION);
        assert_eq!(decoded.schema_version, 4);
        assert_eq!(decoded.device.machine.source_model, "AirSense11 AutoSet");
    }

    #[test]
    fn schema_v2_machine_identity_defaults_the_new_source_model() {
        let legacy = json!({
            "brand": "ResMed",
            "model": "AirSense 10 AutoSet",
            "model_number": "37028",
            "serial": "123",
            "series": "AirSense 10"
        });

        let decoded: MachineInfo =
            serde_json::from_value(legacy).expect("deserialize schema-v2 machine identity");
        assert_eq!(decoded.model, "AirSense 10 AutoSet");
        assert!(decoded.source_model.is_empty());
    }

    #[test]
    fn session_timestamp_preserves_normalization_inputs() {
        let device_local = DeviceLocalDateTime {
            year: 2026,
            month: 2,
            day: 1,
            hour: 6,
            minute: 15,
            second: 0,
            millisecond: 0,
        };
        let timestamp = SessionTimestamp {
            normalized_utc_unix_ms: 1_769_917_500_000,
            device_local_wall_time: "2026-02-01T06:15:00.000".to_owned(),
            device_local: Some(device_local),
            applied_utc_offset_seconds: Some(7 * 60 * 60),
            device_clock_correction_ms: -500,
            timezone_basis: Some("fixed-offset:+07:00".to_owned()),
        };

        let encoded = serde_json::to_value(&timestamp).expect("serialize timestamp");
        assert_eq!(encoded["device_local_wall_time"], "2026-02-01T06:15:00.000");
        assert_eq!(encoded["device_local"]["year"], 2026);
        assert_eq!(encoded["device_local"]["month"], 2);
        assert_eq!(encoded["applied_utc_offset_seconds"], 25_200);
        assert_eq!(encoded["device_clock_correction_ms"], -500);
        assert_eq!(encoded["timezone_basis"], "fixed-offset:+07:00");
        assert_eq!(
            serde_json::from_value::<SessionTimestamp>(encoded).expect("deserialize timestamp"),
            timestamp
        );
    }

    #[test]
    fn event_missing_duration_is_distinct_from_reported_zero_duration() {
        let missing = Event {
            source_key: "event-missing-duration".to_owned(),
            start_time_unix_ms: 1_000,
            duration_ms: None,
            value: None,
        };
        let zero = Event {
            source_key: "event-zero-duration".to_owned(),
            start_time_unix_ms: 2_000,
            duration_ms: Some(0),
            value: None,
        };

        let missing_json = serde_json::to_value(&missing).expect("serialize missing duration");
        let zero_json = serde_json::to_value(&zero).expect("serialize zero duration");
        assert!(missing_json.get("duration_ms").is_none());
        assert_eq!(zero_json["duration_ms"], 0);
        assert_eq!(
            serde_json::from_value::<Event>(missing_json).expect("round-trip missing duration"),
            missing
        );
        assert_eq!(
            serde_json::from_value::<Event>(zero_json).expect("round-trip zero duration"),
            zero
        );

        let legacy_missing: Event = serde_json::from_value(json!({
            "start_time_unix_ms": 3_000,
            "value": null
        }))
        .expect("read event without schema-v4 fields");
        assert!(legacy_missing.source_key.is_empty());
        assert_eq!(legacy_missing.duration_ms, None);
    }

    #[test]
    fn schema_v3_session_defaults_new_schema_v4_fields_conservatively() {
        let legacy = json!({
            "id": "legacy-session",
            "start_time": {
                "normalized_utc_unix_ms": 1_000,
                "device_local_wall_time": "2026-01-01T12:00:00.000",
                "applied_utc_offset_seconds": 0,
                "device_clock_correction_ms": 0
            },
            "end_time": {
                "normalized_utc_unix_ms": 2_000,
                "device_local_wall_time": "2026-01-01T12:00:01.000",
                "applied_utc_offset_seconds": 0,
                "device_clock_correction_ms": 0
            },
            "channels": [],
            "waveforms": [{
                "channel_id": "pap.series.flow_rate",
                "start_time_unix_ms": 1_000,
                "sample_interval_ms": 40.0,
                "samples": [1.0, 2.0]
            }],
            "event_series": [{
                "channel_id": "pap.event.hypopnea",
                "events": [{
                    "start_time_unix_ms": 1_500,
                    "duration_ms": 0,
                    "value": null
                }]
            }],
            "settings": [{
                "key": "pap.setting.mode",
                "label": "Mode",
                "value": {"type": "text", "value": "APAP"}
            }],
            "summary": {
                "usage_ms": 1_000,
                "metrics": []
            }
        });

        let decoded: Session =
            serde_json::from_value(legacy).expect("deserialize schema-v3 session");

        assert!(decoded.source_key.is_empty());
        assert!(decoded.therapy_day.is_empty());
        assert_eq!(decoded.data_kind, SessionDataKind::Partial);
        assert!(decoded.slices.is_empty());
        assert!(decoded.start_time.device_local.is_none());
        assert!(decoded.start_time.timezone_basis.is_none());
        assert!(decoded.waveforms[0].source_key.is_empty());
        assert!(decoded.waveforms[0].source_encoding.is_none());
        assert!(decoded.event_series[0].events[0].source_key.is_empty());
        assert_eq!(decoded.event_series[0].events[0].duration_ms, Some(0));
        assert_eq!(decoded.settings[0].origin, ValueOrigin::Inferred);
    }

    #[test]
    fn schema_v4_session_round_trips_source_and_provenance_metadata() {
        let local_start = DeviceLocalDateTime {
            year: 2026,
            month: 1,
            day: 1,
            hour: 22,
            minute: 0,
            second: 0,
            millisecond: 0,
        };
        let start = SessionTimestamp {
            normalized_utc_unix_ms: 1_767_282_400_000,
            device_local_wall_time: "2026-01-01T22:00:00.000".to_owned(),
            device_local: Some(local_start),
            applied_utc_offset_seconds: Some(0),
            device_clock_correction_ms: 0,
            timezone_basis: Some("fixture:utc".to_owned()),
        };
        let end = SessionTimestamp {
            normalized_utc_unix_ms: 1_767_282_401_000,
            device_local_wall_time: "2026-01-01T22:00:01.000".to_owned(),
            device_local: Some(DeviceLocalDateTime {
                second: 1,
                ..local_start
            }),
            applied_utc_offset_seconds: Some(0),
            device_clock_correction_ms: 0,
            timezone_basis: Some("fixture:utc".to_owned()),
        };
        let session = Session {
            id: "session-20260101-220000".to_owned(),
            source_key: "session-source-01".to_owned(),
            therapy_day: "2026-01-01".to_owned(),
            data_kind: SessionDataKind::Detailed,
            start_time: start,
            end_time: end,
            slices: vec![TherapySlice {
                source_key: "slice-source-01".to_owned(),
                state: TherapySliceState::MaskOn,
                start_time_unix_ms: 1_767_282_400_000,
                end_time_unix_ms: 1_767_282_401_000,
            }],
            channels: Vec::new(),
            waveforms: vec![WaveformSeries {
                source_key: "waveform-source-01".to_owned(),
                channel_id: "pap.series.flow_rate".to_owned(),
                start_time_unix_ms: 1_767_282_400_000,
                sample_interval_ms: 40.0,
                samples: vec![1.25, -0.5],
                source_encoding: Some(EdfSourceEncoding {
                    digital_minimum: -32_768,
                    digital_maximum: 32_767,
                    physical_minimum: -120.0,
                    physical_maximum: 120.0,
                    samples_per_record: 25,
                    record_duration_seconds: 1.0,
                }),
            }],
            event_series: vec![EventSeries {
                channel_id: "pap.event.hypopnea".to_owned(),
                events: vec![Event {
                    source_key: "event-source-01".to_owned(),
                    start_time_unix_ms: 1_767_282_400_500,
                    duration_ms: None,
                    value: None,
                }],
            }],
            settings: vec![Setting {
                key: "pap.setting.mode".to_owned(),
                label: "Mode".to_owned(),
                unit: None,
                value: SettingValue::Text("APAP".to_owned()),
                origin: ValueOrigin::Derived {
                    algorithm: "fixture-mode-v1".to_owned(),
                },
            }],
            summary: SessionSummary {
                usage_ms: 1_000,
                metrics: Vec::new(),
            },
        };

        let encoded = serde_json::to_value(&session).expect("serialize schema-v4 session");
        assert_eq!(encoded["data_kind"], "detailed");
        assert_eq!(encoded["slices"][0]["state"], "mask_on");
        assert_eq!(encoded["settings"][0]["origin"]["type"], "derived");
        assert_eq!(
            encoded["settings"][0]["origin"]["algorithm"],
            "fixture-mode-v1"
        );
        assert_eq!(
            serde_json::from_value::<Session>(encoded).expect("deserialize schema-v4 session"),
            session
        );
    }
}
