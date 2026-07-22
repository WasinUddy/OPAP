// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import type {
  CreateProfileRequest,
  OpapClient,
  PrepareImportJobRequest,
} from './contracts';
import {
  decodeBootstrap,
  decodeImportJob,
  decodeImportJobs,
  decodePrepareImportJobResponse,
  decodeProfile,
  decodeProfiles,
  decodeSourceInspection,
} from './decode';
import { normalizeApiError } from './errors';
import { isOpaqueSourceId } from './sourceId';

/** Stable names the Rust host must register with `tauri::generate_handler!`. */
export const TAURI_COMMANDS = {
  bootstrap: 'app_bootstrap',
  listProfiles: 'profile_list',
  createProfile: 'profile_create',
  selectNativeSource: 'source_select',
  prepareImportJob: 'import_prepare',
  listImportJobs: 'import_jobs',
  cancelImportJob: 'import_cancel',
} as const;

export type TauriInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

export function createTauriOpapClient(invoke: TauriInvoke): OpapClient {
  async function call<T>(
    command: (typeof TAURI_COMMANDS)[keyof typeof TAURI_COMMANDS],
    decode: (value: unknown) => T,
    args?: Record<string, unknown>,
  ): Promise<T> {
    try {
      return decode(await invoke(command, args));
    } catch (error) {
      throw normalizeApiError(error);
    }
  }

  return {
    runtime: 'tauri',
    bootstrap: () => call(TAURI_COMMANDS.bootstrap, decodeBootstrap),
    listProfiles: () => call(TAURI_COMMANDS.listProfiles, decodeProfiles),
    createProfile: (request: CreateProfileRequest) =>
      call(TAURI_COMMANDS.createProfile, decodeProfile, { request }),
    selectNativeSource: () =>
      call(
        TAURI_COMMANDS.selectNativeSource,
        (value) => (value === null ? null : decodeSourceInspection(value)),
      ),
    prepareImportJob: (request: PrepareImportJobRequest) => {
      if (!isOpaqueSourceId(request.source_id)) {
        return Promise.reject(
          normalizeApiError({
            code: 'invalid_request',
            message: 'Source ID is invalid; select the folder again.',
            retryable: false,
            field: 'source_id',
          }),
        );
      }
      return call(TAURI_COMMANDS.prepareImportJob, decodePrepareImportJobResponse, { request });
    },
    listImportJobs: (profileId: number) =>
      call(TAURI_COMMANDS.listImportJobs, decodeImportJobs, { profileId }),
    getImportJob: async (profileId: number, jobId: number) => {
      const jobs = await call(TAURI_COMMANDS.listImportJobs, decodeImportJobs, { profileId });
      const job = jobs.find((candidate) => candidate.id === jobId);
      if (job === undefined) {
        throw normalizeApiError({
          code: 'job_not_found',
          message: 'The requested import job does not exist.',
          retryable: false,
        });
      }
      return job;
    },
    cancelImportJob: (profileId: number, jobId: number) =>
      call(TAURI_COMMANDS.cancelImportJob, decodeImportJob, { profileId, jobId }),
  };
}
