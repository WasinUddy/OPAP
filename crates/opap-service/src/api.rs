// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Serializable OPAP application API types.

use serde::{Deserialize, Serialize};

/// Current version of the service DTO contract.
pub const API_SCHEMA_VERSION: u16 = 1;

/// Stable reason session import cannot currently run.
pub const SESSION_IMPORT_UNAVAILABLE_REASON: &str = "session_parser_not_implemented";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppBootstrap {
    pub api_schema_version: u16,
    pub import_report_schema_version: u16,
    pub storage_schema_version: i64,
    pub capabilities: AppCapabilities,
    pub importers: Vec<ImporterCapability>,
    pub profiles: Vec<ProfileDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppCapabilities {
    pub profile_management: bool,
    pub source_inspection: bool,
    pub import_job_preparation: bool,
    pub session_import: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterCapability {
    pub id: String,
    pub display_name: String,
    pub source_inspection: bool,
    pub session_import: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileDto {
    pub id: i64,
    pub display_name: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProfileRequest {
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInspection {
    /// Opaque, process-local handle to the native directory capability.
    pub source_id: String,
    pub recognized: bool,
    /// Redacted description suitable for the web view.
    pub source_label: String,
    pub files: u64,
    pub directories: u64,
    pub total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub importer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceDto>,
    pub warnings: Vec<ImportWarningDto>,
    pub session_import: SessionImportCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDto {
    pub brand: String,
    pub model: String,
    pub model_number: String,
    /// At most the final four device serial characters.
    pub serial_suffix: String,
    pub series: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverityDto {
    Info,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportWarningDto {
    pub code: String,
    pub severity: WarningSeverityDto,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionImportCapability {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareImportJobRequest {
    pub profile_id: i64,
    /// Opaque handle returned by native source inspection. No filesystem path
    /// crosses the serialized application boundary.
    pub source_id: String,
    /// Caller-generated idempotency key, scoped to the profile.
    pub request_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareImportJobResponse {
    pub job: ImportJobDto,
    /// False when the request key already referred to the same logical job.
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportJobStatus {
    /// Persisted and cancellable, but waiting for session importer support.
    Blocked,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportJobPhase {
    AwaitingSessionImporter,
    Importing,
    Finished,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportJobCounts {
    pub sessions_created: i64,
    pub sessions_updated: i64,
    pub events_written: i64,
    pub waveform_chunks_written: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportJobDto {
    pub id: i64,
    pub profile_id: i64,
    pub request_key: String,
    pub attempt: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_of_id: Option<i64>,
    /// Opaque persisted source identifier; never a filesystem path.
    pub source_id: String,
    /// Generic redacted source description.
    pub source_label: String,
    pub importer_id: String,
    pub status: ImportJobStatus,
    pub phase: ImportJobPhase,
    /// Time the job record was durably created.
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<i64>,
    pub counts: ImportJobCounts,
    pub can_cancel: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_message: Option<String>,
}
