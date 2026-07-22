// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import {
  type AppBootstrap,
  type DeviceDto,
  type ImportJobCounts,
  type ImportJobDto,
  type ImportJobPhase,
  type ImportJobStatus,
  type ImportWarningDto,
  type ImporterCapability,
  type PrepareImportJobResponse,
  type ProfileDto,
  type SessionImportCapability,
  type SourceInspection,
  type WarningSeverityDto,
  OPAP_API_SCHEMA_VERSION,
} from './contracts';
import { isOpaqueSourceId } from './sourceId';

export function decodeBootstrap(value: unknown): AppBootstrap {
  const object = record(value, 'bootstrap');
  const apiSchemaVersion = integer(object.api_schema_version, 'api_schema_version');
  if (apiSchemaVersion !== OPAP_API_SCHEMA_VERSION) {
    throw new TypeError('Unsupported api_schema_version response field');
  }
  return {
    api_schema_version: apiSchemaVersion,
    import_report_schema_version: integer(object.import_report_schema_version, 'import_report_schema_version'),
    storage_schema_version: integer(object.storage_schema_version, 'storage_schema_version'),
    capabilities: decodeCapabilities(object.capabilities),
    importers: array(object.importers, decodeImporter, 'importers'),
    profiles: decodeProfiles(object.profiles),
  };
}

export function decodeProfiles(value: unknown): ProfileDto[] {
  return array(value, decodeProfile, 'profiles');
}

export function decodeProfile(value: unknown): ProfileDto {
  const object = record(value, 'profile');
  return {
    id: integer(object.id, 'id'),
    display_name: string(object.display_name, 'display_name'),
    created_at_ms: integer(object.created_at_ms, 'created_at_ms'),
    updated_at_ms: integer(object.updated_at_ms, 'updated_at_ms'),
  };
}

export function decodeSourceInspection(value: unknown): SourceInspection {
  const object = record(value, 'source inspection');
  return {
    source_id: opaqueSourceId(object.source_id),
    recognized: boolean(object.recognized, 'recognized'),
    source_label: safeText(object.source_label, 'source_label'),
    files: nonNegativeInteger(object.files, 'files'),
    directories: nonNegativeInteger(object.directories, 'directories'),
    total_bytes: nonNegativeInteger(object.total_bytes, 'total_bytes'),
    ...optional(object, 'importer_id', string),
    ...optional(object, 'device', decodeDevice),
    warnings: array(object.warnings, decodeWarning, 'warnings'),
    session_import: decodeSessionImport(object.session_import),
  };
}

export function decodePrepareImportJobResponse(value: unknown): PrepareImportJobResponse {
  const object = record(value, 'prepared import job');
  return {
    job: decodeImportJob(object.job),
    created: boolean(object.created, 'created'),
  };
}

export function decodeImportJobs(value: unknown): ImportJobDto[] {
  return array(value, decodeImportJob, 'import jobs');
}

export function decodeImportJob(value: unknown): ImportJobDto {
  const object = record(value, 'import job');
  return {
    id: integer(object.id, 'id'),
    profile_id: integer(object.profile_id, 'profile_id'),
    request_key: string(object.request_key, 'request_key'),
    attempt: integer(object.attempt, 'attempt'),
    ...optional(object, 'retry_of_id', integer),
    source_id: opaqueSourceId(object.source_id),
    source_label: safeText(object.source_label, 'source_label'),
    importer_id: string(object.importer_id, 'importer_id'),
    status: enumeration(object.status, IMPORT_JOB_STATUSES, 'status'),
    phase: enumeration(object.phase, IMPORT_JOB_PHASES, 'phase'),
    created_at_ms: integer(object.created_at_ms, 'created_at_ms'),
    updated_at_ms: integer(object.updated_at_ms, 'updated_at_ms'),
    ...optional(object, 'started_at_ms', integer),
    ...optional(object, 'finished_at_ms', integer),
    counts: decodeCounts(object.counts),
    can_cancel: boolean(object.can_cancel, 'can_cancel'),
    ...optional(object, 'unavailable_reason', safeText),
    ...optional(object, 'failure_message', safeText),
  };
}

function decodeCapabilities(value: unknown): AppBootstrap['capabilities'] {
  const object = record(value, 'capabilities');
  return {
    profile_management: boolean(object.profile_management, 'profile_management'),
    source_inspection: boolean(object.source_inspection, 'source_inspection'),
    import_job_preparation: boolean(object.import_job_preparation, 'import_job_preparation'),
    session_import: boolean(object.session_import, 'session_import'),
  };
}

function decodeImporter(value: unknown): ImporterCapability {
  const object = record(value, 'importer');
  return {
    id: string(object.id, 'id'),
    display_name: string(object.display_name, 'display_name'),
    source_inspection: boolean(object.source_inspection, 'source_inspection'),
    session_import: boolean(object.session_import, 'session_import'),
    ...optional(object, 'unavailable_reason', safeText),
  };
}

function decodeDevice(value: unknown): DeviceDto {
  const object = record(value, 'device');
  return {
    brand: string(object.brand, 'brand'),
    model: string(object.model, 'model'),
    model_number: string(object.model_number, 'model_number'),
    serial_suffix: serialSuffix(object.serial_suffix),
    series: string(object.series, 'series'),
  };
}

function decodeWarning(value: unknown): ImportWarningDto {
  const object = record(value, 'warning');
  return {
    code: string(object.code, 'code'),
    severity: enumeration(object.severity, WARNING_SEVERITIES, 'severity'),
    message: safeText(object.message, 'message'),
  };
}

function decodeSessionImport(value: unknown): SessionImportCapability {
  const object = record(value, 'session import capability');
  return {
    available: boolean(object.available, 'available'),
    ...optional(object, 'unavailable_reason', safeText),
  };
}

function decodeCounts(value: unknown): ImportJobCounts {
  const object = record(value, 'import job counts');
  return {
    sessions_created: integer(object.sessions_created, 'sessions_created'),
    sessions_updated: integer(object.sessions_updated, 'sessions_updated'),
    events_written: integer(object.events_written, 'events_written'),
    waveform_chunks_written: integer(object.waveform_chunks_written, 'waveform_chunks_written'),
  };
}

const WARNING_SEVERITIES = ['info', 'warning'] as const satisfies readonly WarningSeverityDto[];
const IMPORT_JOB_STATUSES = [
  'blocked',
  'running',
  'completed',
  'failed',
  'cancelled',
] as const satisfies readonly ImportJobStatus[];
const IMPORT_JOB_PHASES = [
  'awaiting_session_importer',
  'importing',
  'finished',
] as const satisfies readonly ImportJobPhase[];

function record(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new TypeError(`Invalid ${name} response`);
  }
  return value as Record<string, unknown>;
}

function string(value: unknown, name: string): string {
  if (typeof value !== 'string') throw new TypeError(`Invalid ${name} response field`);
  return value;
}

function safeText(value: unknown, name: string): string {
  const decoded = string(value, name);
  const containsControlCharacter = [...decoded].some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint <= 0x1f || codePoint === 0x7f;
  });
  // Canonical service display strings currently contain no path separators.
  // Reject both separators to fail closed for Unix, Windows, UNC, URL-shaped,
  // parenthesized, and bracketed paths without attempting fragile redaction.
  const containsPathSeparator = decoded.includes('/') || decoded.includes('\\');
  if (containsControlCharacter || containsPathSeparator) {
    throw new TypeError(`Unsafe ${name} response field`);
  }
  return decoded;
}

function serialSuffix(value: unknown): string {
  const decoded = safeText(value, 'serial_suffix');
  if ([...decoded].length > 4) throw new TypeError('Invalid serial_suffix response field');
  return decoded;
}

function boolean(value: unknown, name: string): boolean {
  if (typeof value !== 'boolean') throw new TypeError(`Invalid ${name} response field`);
  return value;
}

function integer(value: unknown, name: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value)) {
    throw new TypeError(`Invalid ${name} response field`);
  }
  return value;
}

function nonNegativeInteger(value: unknown, name: string): number {
  const decoded = integer(value, name);
  if (decoded < 0) throw new TypeError(`Invalid ${name} response field`);
  return decoded;
}

function opaqueSourceId(value: unknown): string {
  if (!isOpaqueSourceId(value)) throw new TypeError('Invalid source_id response field');
  return value;
}

function enumeration<const T extends string>(value: unknown, values: readonly T[], name: string): T {
  if (typeof value !== 'string' || !values.includes(value as T)) {
    throw new TypeError(`Invalid ${name} response field`);
  }
  return value as T;
}

function array<T>(value: unknown, decode: (item: unknown) => T, name: string): T[] {
  if (!Array.isArray(value)) throw new TypeError(`Invalid ${name} response`);
  return value.map(decode);
}

function optional<T>(
  object: Record<string, unknown>,
  key: string,
  decode: (value: unknown, name: string) => T,
): { [property: string]: T } | Record<string, never> {
  const value = object[key];
  return value === undefined ? {} : { [key]: decode(value, key) };
}
