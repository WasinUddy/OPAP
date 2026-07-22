// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import { createDemoOpapClient } from './demoClient';

describe('demo OPAP client', () => {
  it('marks every preview boundary as demo or fabricated', async () => {
    const client = createDemoOpapClient();
    const bootstrap = await client.bootstrap();
    const source = await client.selectNativeSource();

    expect(client.runtime).toBe('demo');
    expect(bootstrap.profiles[0].display_name).toMatch(/demo|sample/i);
    expect(bootstrap.importers[0].display_name).toMatch(/demo|fabricated/i);
    expect(bootstrap.importers[0].unavailable_reason).toMatch(/demo/i);
    expect(source?.source_label).toMatch(/demo|no folder was read/i);
    expect(source?.warnings[0].message).toMatch(/fabricated sample data/i);
    expect(JSON.stringify(source)).not.toMatch(/[/\\](Users|home|Volumes|mnt)[/\\]/i);
  });

  it('keeps demo mutations in memory and labels created profiles', async () => {
    const client = createDemoOpapClient();
    const profile = await client.createProfile({ display_name: 'Night shift' });

    expect(profile.display_name).toBe('Demo · Night shift');
    expect(await client.listProfiles()).toContainEqual(profile);
  });

  it('supports idempotent demo job preparation and cancellation without writing therapy data', async () => {
    const client = createDemoOpapClient();
    const source = await client.selectNativeSource();
    if (!source) throw new Error('The built-in demo source must exist.');
    const request = { profile_id: 1, source_id: source.source_id };

    const first = await client.prepareImportJob(request);
    const repeated = await client.prepareImportJob(request);
    const cancelled = await client.cancelImportJob(1, first.job.id);

    expect(first.created).toBe(true);
    expect(first.job).toMatchObject({
      status: 'blocked',
      can_cancel: true,
      counts: {
        sessions_created: 0,
        sessions_updated: 0,
        events_written: 0,
        waveform_chunks_written: 0,
      },
    });
    expect(repeated).toEqual({ job: first.job, created: false });
    expect(cancelled).toMatchObject({ status: 'cancelled', can_cancel: false });
    expect(await client.getImportJob(1, first.job.id)).toEqual(cancelled);
    expect(await client.listImportJobs(1)).toEqual([cancelled]);
  });

  it('returns stable typed errors for invalid demo requests', async () => {
    const client = createDemoOpapClient();

    await expect(client.createProfile({ display_name: '  ' })).rejects.toMatchObject({
      code: 'invalid_request',
      field: 'display_name',
    });
    await expect(client.listImportJobs(999)).rejects.toMatchObject({ code: 'profile_not_found' });
    await expect(
      client.prepareImportJob({ profile_id: 1, source_id: 'not-a-handle' }),
    ).rejects.toMatchObject({ code: 'source_unavailable', field: 'source_id' });
  });
});
