// Copyright (C) 2011-2018 Mark Watkins
// Copyright (C) 2019-2026 The OSCAR Team
// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Ported and modified from OSCAR-SQL concepts:
// https://gitlab.com/CrimsonNape/OSCAR-SQL
// Upstream commit: 3741e5b423e4b5796c51a9d447e83b2525963d50
// Relevant upstream files: oscar/SleepLib/machine.h,
// oscar/SleepLib/session.h, oscar/SleepLib/eventlist.h
// Modified: 2026-07-22

//! Serializable domain types shared by importers and application frontends.
//!
//! These types deliberately avoid native filesystem and UI concepts. Their
//! serialized representation is the contract that a desktop host, command-line
//! client, or future WebAssembly wrapper consumes.

use serde::{Deserialize, Serialize};

/// Current version of the serialized import report contract.
pub const IMPORT_SCHEMA_VERSION: u16 = 2;

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
    /// Human-readable product name.
    pub model: String,
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
    /// Identifier of the corresponding [`ChannelMetadata`].
    pub channel_id: String,
    /// UTC time of the first sample.
    pub start_time_unix_ms: UnixMillis,
    /// Milliseconds between consecutive samples.
    pub sample_interval_ms: f64,
    /// Samples in chronological order, in the channel's declared unit.
    pub samples: Vec<f32>,
}

/// One discrete event in an [`EventSeries`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// UTC time at which the event starts.
    pub start_time_unix_ms: UnixMillis,
    /// Event duration. Instantaneous events use zero.
    pub duration_ms: u64,
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
    /// Signed seconds by which device-local time was ahead of UTC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_utc_offset_seconds: Option<i32>,
    /// Signed correction added to the device clock before UTC normalization.
    pub device_clock_correction_ms: i64,
}

/// A single contiguous CPAP therapy session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Stable importer-derived identifier used for idempotent re-imports.
    pub id: String,
    /// Start boundary, inclusive, with device-clock provenance.
    pub start_time: SessionTimestamp,
    /// End boundary, exclusive, with device-clock provenance.
    pub end_time: SessionTimestamp,
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
    }

    #[test]
    fn session_timestamp_preserves_normalization_inputs() {
        let timestamp = SessionTimestamp {
            normalized_utc_unix_ms: 1_769_917_500_000,
            device_local_wall_time: "2026-02-01T06:15:00.000".to_owned(),
            applied_utc_offset_seconds: Some(7 * 60 * 60),
            device_clock_correction_ms: -500,
        };

        let encoded = serde_json::to_value(&timestamp).expect("serialize timestamp");
        assert_eq!(encoded["device_local_wall_time"], "2026-02-01T06:15:00.000");
        assert_eq!(encoded["applied_utc_offset_seconds"], 25_200);
        assert_eq!(encoded["device_clock_correction_ms"], -500);
        assert_eq!(
            serde_json::from_value::<SessionTimestamp>(encoded).expect("deserialize timestamp"),
            timestamp
        );
    }
}
