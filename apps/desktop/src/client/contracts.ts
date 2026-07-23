// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

/**
 * Renderer-facing DTOs for `opap-service` API schema version 2.
 *
 * Property names intentionally match the Rust/Serde wire format exactly. Keep
 * this file in lockstep with `crates/opap-service/src/api.rs` and
 * `crates/opap-service/src/error.rs`.
 */

export const OPAP_API_SCHEMA_VERSION = 2 as const;

export interface AppBootstrap {
  api_schema_version: number;
  import_report_schema_version: number;
  storage_schema_version: number;
  capabilities: AppCapabilities;
  importers: ImporterCapability[];
  profiles: ProfileDto[];
}

export interface AppCapabilities {
  profile_management: boolean;
  source_inspection: boolean;
  import_job_preparation: boolean;
  session_import: boolean;
}

export interface ImporterCapability {
  id: string;
  display_name: string;
  source_inspection: boolean;
  session_import: boolean;
  unavailable_reason?: string;
}

export interface ProfileDto {
  id: number;
  display_name: string;
  created_at_ms: number;
  updated_at_ms: number;
}

export interface CreateProfileRequest {
  display_name: string;
}

export interface SourceInspection {
  /** Opaque, process-local handle. It is never a filesystem path. */
  source_id: string;
  recognized: boolean;
  source_label: string;
  files: number;
  directories: number;
  total_bytes: number;
  importer_id?: string;
  device?: DeviceDto;
  warnings: ImportWarningDto[];
  session_import: SessionImportCapability;
}

export interface DeviceDto {
  brand: string;
  model: string;
  model_number: string;
  /** At most the final four characters of the serial number. */
  serial_suffix: string;
  series: string;
}

export type WarningSeverityDto = 'info' | 'warning';

export interface ImportWarningDto {
  code: string;
  severity: WarningSeverityDto;
  message: string;
}

export interface SessionImportCapability {
  available: boolean;
  unavailable_reason?: string;
}

export interface PrepareImportJobRequest {
  profile_id: number;
  /** Opaque handle returned by native source selection. */
  source_id: string;
}

export interface PrepareImportJobResponse {
  job: ImportJobDto;
  created: boolean;
}

export type ImportJobStatus = 'blocked' | 'running' | 'completed' | 'failed' | 'cancelled';

export type ImportJobPhase = 'awaiting_session_importer' | 'importing' | 'finished';

export interface ImportJobCounts {
  sessions_created: number;
  sessions_updated: number;
  events_written: number;
  waveform_chunks_written: number;
}

export interface ImportJobDto {
  id: number;
  profile_id: number;
  attempt: number;
  retry_of_id?: number;
  source_id: string;
  source_label: string;
  importer_id: string;
  status: ImportJobStatus;
  phase: ImportJobPhase;
  created_at_ms: number;
  updated_at_ms: number;
  started_at_ms?: number;
  finished_at_ms?: number;
  counts: ImportJobCounts;
  can_cancel: boolean;
  unavailable_reason?: string;
  failure_message?: string;
}

export const API_ERROR_CODES = [
  'invalid_request',
  'profile_not_found',
  'job_not_found',
  'conflict',
  'source_unavailable',
  'source_path_invalid',
  'source_not_supported',
  'source_data_invalid',
  'source_size_limit_exceeded',
  'capability_unavailable',
  'job_not_cancellable',
  'storage_unavailable',
  'internal',
] as const;

export type ApiErrorCode = (typeof API_ERROR_CODES)[number];

export interface ApiError {
  code: ApiErrorCode;
  message: string;
  retryable: boolean;
  field?: string;
}

export type ClientRuntime = 'tauri' | 'demo';

export interface OpapClient {
  /** Makes demo data impossible to confuse with native service data. */
  readonly runtime: ClientRuntime;

  bootstrap(): Promise<AppBootstrap>;
  listProfiles(): Promise<ProfileDto[]>;
  createProfile(request: CreateProfileRequest): Promise<ProfileDto>;

  /**
   * Opens the native directory chooser and inspects the selected source.
   * No path argument or path-bearing result crosses the renderer boundary.
   * `null` means the person cancelled the native chooser.
   */
  selectNativeSource(): Promise<SourceInspection | null>;

  prepareImportJob(request: PrepareImportJobRequest): Promise<PrepareImportJobResponse>;
  listImportJobs(profileId: number): Promise<ImportJobDto[]>;
  getImportJob(profileId: number, jobId: number): Promise<ImportJobDto>;
  cancelImportJob(profileId: number, jobId: number): Promise<ImportJobDto>;
}
