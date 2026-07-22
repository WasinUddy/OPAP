// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import type {
  AppBootstrap,
  CreateProfileRequest,
  ImportJobDto,
  OpapClient,
  PrepareImportJobRequest,
  ProfileDto,
  SourceInspection,
} from './contracts';
import { OpapApiError } from './errors';

const DEMO_NOW_MS = Date.UTC(2026, 6, 21, 12, 0, 0);
const DEMO_SOURCE_ID = 'opap-source:00000000000000000000000000000000';
const DEMO_ONLY_REASON = 'demo_only_no_therapy_data_is_read_or_written';

/**
 * Explicit browser preview adapter. Every record is marked as demo/sample and
 * all state lives only in this instance's memory.
 */
export function createDemoOpapClient(): OpapClient {
  let nextProfileId = 2;
  let nextJobId = 1;
  const profiles: ProfileDto[] = [demoProfile()];
  const jobs: ImportJobDto[] = [];

  return {
    runtime: 'demo',
    async bootstrap(): Promise<AppBootstrap> {
      return {
        api_schema_version: 1,
        import_report_schema_version: 1,
        storage_schema_version: 0,
        capabilities: {
          profile_management: true,
          source_inspection: true,
          import_job_preparation: true,
          session_import: false,
        },
        importers: [
          {
            id: 'resmed',
            display_name: 'Demo ResMed SD card · fabricated sample',
            source_inspection: true,
            session_import: false,
            unavailable_reason: DEMO_ONLY_REASON,
          },
        ],
        profiles: clone(profiles),
      };
    },
    async listProfiles() {
      return clone(profiles);
    },
    async createProfile(request: CreateProfileRequest) {
      const requestedName = request.display_name.trim();
      if (!requestedName) {
        throw new OpapApiError({
          code: 'invalid_request',
          message: 'Demo profile display name is required.',
          retryable: false,
          field: 'display_name',
        });
      }
      const profile: ProfileDto = {
        id: nextProfileId++,
        display_name: `Demo · ${requestedName}`,
        created_at_ms: DEMO_NOW_MS,
        updated_at_ms: DEMO_NOW_MS,
      };
      profiles.push(profile);
      return clone(profile);
    },
    async selectNativeSource() {
      return clone(demoInspection());
    },
    async prepareImportJob(request: PrepareImportJobRequest) {
      requireDemoProfile(profiles, request.profile_id);
      if (request.source_id !== DEMO_SOURCE_ID) {
        throw new OpapApiError({
          code: 'source_unavailable',
          message: 'Select the built-in demo source again.',
          retryable: false,
          field: 'source_id',
        });
      }
      const existing = jobs.find(
        (job) => job.profile_id === request.profile_id && job.request_key === request.request_key,
      );
      if (existing) return { job: clone(existing), created: false };

      const job = demoJob(nextJobId++, request);
      jobs.push(job);
      return { job: clone(job), created: true };
    },
    async listImportJobs(profileId: number) {
      requireDemoProfile(profiles, profileId);
      return clone(jobs.filter((job) => job.profile_id === profileId));
    },
    async getImportJob(profileId: number, jobId: number) {
      return clone(requireDemoJob(jobs, profileId, jobId));
    },
    async cancelImportJob(profileId: number, jobId: number) {
      const job = requireDemoJob(jobs, profileId, jobId);
      if (!job.can_cancel) {
        throw new OpapApiError({
          code: 'job_not_cancellable',
          message: 'The demo import job is already in a terminal state.',
          retryable: false,
        });
      }
      job.status = 'cancelled';
      job.phase = 'finished';
      job.can_cancel = false;
      job.finished_at_ms = DEMO_NOW_MS;
      job.updated_at_ms = DEMO_NOW_MS;
      delete job.unavailable_reason;
      return clone(job);
    },
  };
}

function demoProfile(): ProfileDto {
  return {
    id: 1,
    display_name: 'Demo profile · fabricated sample data',
    created_at_ms: DEMO_NOW_MS,
    updated_at_ms: DEMO_NOW_MS,
  };
}

function demoInspection(): SourceInspection {
  return {
    source_id: DEMO_SOURCE_ID,
    recognized: true,
    source_label: 'Built-in demo source · no folder was read',
    files: 42,
    directories: 3,
    total_bytes: 1_234_567,
    importer_id: 'resmed',
    device: {
      brand: 'ResMed · demo',
      model: 'AirSense 10 · fabricated sample',
      model_number: 'DEMO',
      serial_suffix: 'DEMO',
      series: 'Demo series',
    },
    warnings: [
      {
        code: 'demo_data',
        severity: 'info',
        message: 'This is fabricated sample data; no CPAP card was read.',
      },
    ],
    session_import: { available: false, unavailable_reason: DEMO_ONLY_REASON },
  };
}

function demoJob(id: number, request: PrepareImportJobRequest): ImportJobDto {
  return {
    id,
    profile_id: request.profile_id,
    request_key: request.request_key,
    attempt: 1,
    source_id: DEMO_SOURCE_ID,
    source_label: 'Built-in demo source · fabricated sample',
    importer_id: 'resmed',
    status: 'blocked',
    phase: 'awaiting_session_importer',
    created_at_ms: DEMO_NOW_MS,
    updated_at_ms: DEMO_NOW_MS,
    counts: {
      sessions_created: 0,
      sessions_updated: 0,
      events_written: 0,
      waveform_chunks_written: 0,
    },
    can_cancel: true,
    unavailable_reason: DEMO_ONLY_REASON,
  };
}

function requireDemoProfile(profiles: ProfileDto[], profileId: number): ProfileDto {
  const profile = profiles.find((candidate) => candidate.id === profileId);
  if (!profile) {
    throw new OpapApiError({
      code: 'profile_not_found',
      message: 'The requested demo profile does not exist.',
      retryable: false,
    });
  }
  return profile;
}

function requireDemoJob(jobs: ImportJobDto[], profileId: number, jobId: number): ImportJobDto {
  const job = jobs.find((candidate) => candidate.profile_id === profileId && candidate.id === jobId);
  if (!job) {
    throw new OpapApiError({
      code: 'job_not_found',
      message: 'The requested demo import job does not exist.',
      retryable: false,
    });
  }
  return job;
}

function clone<T>(value: T): T {
  return structuredClone(value);
}
