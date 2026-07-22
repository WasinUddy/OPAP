// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import { OpapApiError } from './errors';
import { createTauriOpapClient, TAURI_COMMANDS, type TauriInvoke } from './tauriClient';

const PROFILE = {
  id: 7,
  display_name: 'Local sleeper',
  created_at_ms: 1_721_562_000_000,
  updated_at_ms: 1_721_562_000_000,
};

const INSPECTION = {
  source_id: 'opap-source:1234567890abcdef1234567890abcdef',
  recognized: true,
  source_label: 'ResMed SD card',
  files: 12,
  directories: 2,
  total_bytes: 4096,
  importer_id: 'resmed',
  device: {
    brand: 'ResMed',
    model: 'AirSense 10',
    model_number: '37028',
    serial_suffix: '1234',
    series: 'AirSense',
  },
  warnings: [{ code: 'clock', severity: 'warning', message: 'Check the device clock.' }],
  session_import: { available: false, unavailable_reason: 'session_parser_not_implemented' },
};

const JOB = {
  id: 11,
  profile_id: 7,
  request_key: 'request-1',
  attempt: 1,
  source_id: INSPECTION.source_id,
  source_label: 'ResMed SD card',
  importer_id: 'resmed',
  status: 'blocked',
  phase: 'awaiting_session_importer',
  created_at_ms: 1_721_562_000_000,
  updated_at_ms: 1_721_562_000_000,
  counts: {
    sessions_created: 0,
    sessions_updated: 0,
    events_written: 0,
    waveform_chunks_written: 0,
  },
  can_cancel: true,
  unavailable_reason: 'session_parser_not_implemented',
};

const BOOTSTRAP = {
  api_schema_version: 1,
  import_report_schema_version: 1,
  storage_schema_version: 4,
  capabilities: {
    profile_management: true,
    source_inspection: true,
    import_job_preparation: true,
    session_import: false,
  },
  importers: [
    {
      id: 'resmed',
      display_name: 'ResMed SD card',
      source_inspection: true,
      session_import: false,
      unavailable_reason: 'session_parser_not_implemented',
    },
  ],
  profiles: [PROFILE],
};

describe('Tauri OPAP client', () => {
  it('uses every stable command and its documented argument envelope', async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const invoke: TauriInvoke = async (command, args) => {
      calls.push({ command, ...(args === undefined ? {} : { args }) });
      switch (command) {
        case TAURI_COMMANDS.bootstrap:
          return BOOTSTRAP;
        case TAURI_COMMANDS.listProfiles:
          return [PROFILE];
        case TAURI_COMMANDS.createProfile:
          return PROFILE;
        case TAURI_COMMANDS.selectNativeSource:
          return INSPECTION;
        case TAURI_COMMANDS.prepareImportJob:
          return { job: JOB, created: true };
        case TAURI_COMMANDS.listImportJobs:
          return [JOB];
        case TAURI_COMMANDS.cancelImportJob:
          return JOB;
        default:
          throw new Error(`Unexpected command: ${command}`);
      }
    };
    const client = createTauriOpapClient(invoke);

    expect(client.runtime).toBe('tauri');
    await client.bootstrap();
    await client.listProfiles();
    await client.createProfile({ display_name: 'Local sleeper' });
    await client.selectNativeSource();
    await client.prepareImportJob({
      profile_id: 7,
      source_id: INSPECTION.source_id,
      request_key: 'request-1',
    });
    await client.listImportJobs(7);
    await client.getImportJob(7, 11);
    await client.cancelImportJob(7, 11);

    expect(calls).toEqual([
      { command: 'app_bootstrap' },
      { command: 'profile_list' },
      { command: 'profile_create', args: { request: { display_name: 'Local sleeper' } } },
      { command: 'source_select' },
      {
        command: 'import_prepare',
        args: {
          request: {
            profile_id: 7,
            source_id: INSPECTION.source_id,
            request_key: 'request-1',
          },
        },
      },
      { command: 'import_jobs', args: { profileId: 7 } },
      { command: 'import_jobs', args: { profileId: 7 } },
      { command: 'import_cancel', args: { profileId: 7, jobId: 11 } },
    ]);
  });

  it('never sends a path-bearing native source selection request', async () => {
    const invoke = vi.fn<TauriInvoke>().mockResolvedValue(null);
    const client = createTauriOpapClient(invoke);

    await expect(client.selectNativeSource()).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith(TAURI_COMMANDS.selectNativeSource, undefined);

    const [, args] = invoke.mock.calls[0];
    expect(args).toBeUndefined();
  });

  it('rejects path-bearing source handles before invoking the native host', async () => {
    const invoke = vi.fn<TauriInvoke>();
    const client = createTauriOpapClient(invoke);

    await expect(
      client.prepareImportJob({
        profile_id: 7,
        source_id: '/Users/person/private/SDCARD',
        request_key: 'request-1',
      }),
    ).rejects.toMatchObject({ code: 'invalid_request', field: 'source_id' });
    await expect(
      client.prepareImportJob({
        profile_id: 7,
        source_id: INSPECTION.source_id,
        request_key: '../../private/card',
      }),
    ).rejects.toMatchObject({ code: 'invalid_request', field: 'request_key' });
    expect(invoke).not.toHaveBeenCalled();
  });

  it('reconstructs DTOs and drops unexpected native fields, including paths', async () => {
    const invoke: TauriInvoke = async () => ({
      ...INSPECTION,
      path: '/Users/person/private/SDCARD',
      source_path: '/Users/person/private/SDCARD',
      device: { ...INSPECTION.device, serial: '12345678901234' },
    });

    const inspection = await createTauriOpapClient(invoke).selectNativeSource();

    expect(inspection).toEqual(INSPECTION);
    expect(inspection).not.toHaveProperty('path');
    expect(inspection).not.toHaveProperty('source_path');
    expect(inspection?.device).not.toHaveProperty('serial');
  });

  it('preserves a valid service ApiError as a typed OpapApiError', async () => {
    const invoke: TauriInvoke = async () =>
      Promise.reject({
        code: 'invalid_request',
        message: 'profile display name is required',
        retryable: false,
        field: 'display_name',
      });

    const promise = createTauriOpapClient(invoke).createProfile({ display_name: '' });

    await expect(promise).rejects.toMatchObject({
      name: 'OpapApiError',
      code: 'invalid_request',
      message: 'The request is invalid.',
      retryable: false,
      field: 'display_name',
    });
  });

  it('redacts malformed native errors instead of exposing possible paths', async () => {
    const invoke: TauriInvoke = async () => Promise.reject('/Users/person/private/card could not open');

    const promise = createTauriOpapClient(invoke).bootstrap();

    await expect(promise).rejects.toEqual(
      new OpapApiError({
        code: 'internal',
        message: 'The native OPAP service returned an unexpected error.',
        retryable: false,
      }),
    );
    await expect(promise).rejects.not.toHaveProperty('message', expect.stringContaining('/Users'));
  });

  it('rejects malformed responses instead of trusting the native payload', async () => {
    const invoke: TauriInvoke = async () => ({ ...PROFILE, id: Number.MAX_SAFE_INTEGER + 1 });

    await expect(createTauriOpapClient(invoke).createProfile({ display_name: 'Sleeper' })).rejects.toMatchObject({
      code: 'internal',
      retryable: false,
    });
  });

  it('fails closed on unsupported API versions and non-Serde optional nulls', async () => {
    const unsupportedVersion: TauriInvoke = async () => ({ ...BOOTSTRAP, api_schema_version: 2 });
    await expect(createTauriOpapClient(unsupportedVersion).bootstrap()).rejects.toMatchObject({
      code: 'internal',
    });

    const nullOptional: TauriInvoke = async () => ({ ...INSPECTION, importer_id: null });
    await expect(createTauriOpapClient(nullOptional).selectNativeSource()).rejects.toMatchObject({
      code: 'internal',
    });
  });

  it('rejects path-bearing canonical text and overlong serial suffixes', async () => {
    const unsafeLabel: TauriInvoke = async () => ({
      ...INSPECTION,
      source_label: '/Users/alice/private/card',
    });
    await expect(createTauriOpapClient(unsafeLabel).selectNativeSource()).rejects.toMatchObject({
      code: 'internal',
    });

    const unsafeWarning: TauriInvoke = async () => ({
      ...INSPECTION,
      warnings: [{ code: 'unsafe', severity: 'warning', message: 'Read C:\\private\\card' }],
    });
    await expect(createTauriOpapClient(unsafeWarning).selectNativeSource()).rejects.toMatchObject({
      code: 'internal',
    });

    for (const source_label of [
      'Could not open (/Users/alice/private/card)',
      'Read [/home/alice/card]',
      'Read (C:\\private\\card)',
    ]) {
      const punctuatedPath: TauriInvoke = async () => ({ ...INSPECTION, source_label });
      await expect(createTauriOpapClient(punctuatedPath).selectNativeSource()).rejects.toMatchObject({
        code: 'internal',
      });
    }

    const fullSerial: TauriInvoke = async () => ({
      ...INSPECTION,
      device: { ...INSPECTION.device, serial_suffix: '123456789' },
    });
    await expect(createTauriOpapClient(fullSerial).selectNativeSource()).rejects.toMatchObject({
      code: 'internal',
    });
  });
});
