// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import { selectOpapClient, type OpapRuntimeWindow } from './runtime';

describe('OPAP client runtime selection', () => {
  it('selects the Tauri client when native IPC is available', async () => {
    const invoke = vi.fn().mockResolvedValue({
      api_schema_version: 2,
      import_report_schema_version: 1,
      storage_schema_version: 4,
      capabilities: {
        profile_management: true,
        source_inspection: true,
        import_job_preparation: true,
        session_import: false,
      },
      importers: [],
      profiles: [],
    });
    const client = selectOpapClient({ __TAURI_INTERNALS__: { invoke } });

    expect(client.runtime).toBe('tauri');
    await client.bootstrap();
    expect(invoke).toHaveBeenCalledWith('app_bootstrap', undefined);
  });

  it.each<OpapRuntimeWindow>([
    { __TAURI_INTERNALS__: {} },
    { __TAURI__: {} },
    { navigator: { userAgent: 'OPAP Tauri/2' } },
  ])('fails closed when Tauri is detectable but IPC is missing', (runtime) => {
    expect(() => selectOpapClient(runtime)).toThrow(/IPC bridge is unavailable/);
  });

  it('selects only the explicitly labelled demo adapter in a normal browser', () => {
    const client = selectOpapClient({ navigator: { userAgent: 'Mozilla/5.0' } });

    expect(client.runtime).toBe('demo');
  });
});
