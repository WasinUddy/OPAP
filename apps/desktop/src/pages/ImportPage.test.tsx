import { MantineProvider } from '@mantine/core';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import type {
  AppBootstrap,
  ImportJobDto,
  OpapClient,
  SourceInspection,
} from '../client';
import { createDemoOpapClient, OpapClientProvider } from '../client';
import { theme } from '../theme';
import { ImportPage } from './ImportPage';

const nativeInspection: SourceInspection = {
  source_id: 'opap-source:1234567890abcdef1234567890abcdef',
  recognized: true,
  source_label: 'Selected CPAP source',
  files: 42,
  directories: 3,
  total_bytes: 1_234_567,
  importer_id: 'resmed',
  device: {
    brand: 'ResMed',
    model: 'AirSense 10 AutoSet',
    model_number: '37028',
    serial_suffix: '9876',
    series: 'AirSense 10',
  },
  warnings: [],
  session_import: { available: false, unavailable_reason: 'session_importer_not_available' },
};

function nativeBootstrap(): AppBootstrap {
  return {
    api_schema_version: 2,
    import_report_schema_version: 1,
    storage_schema_version: 6,
    capabilities: {
      profile_management: true,
      source_inspection: true,
      import_job_preparation: true,
      session_import: false,
    },
    importers: [{
      id: 'resmed',
      display_name: 'ResMed SD card',
      source_inspection: true,
      session_import: false,
      unavailable_reason: 'session_importer_not_available',
    }],
    profiles: [{ id: 7, display_name: 'Local profile', created_at_ms: 1, updated_at_ms: 1 }],
  };
}

function blockedJob(status: ImportJobDto['status'] = 'blocked'): ImportJobDto {
  return {
    id: 12,
    profile_id: 7,
    attempt: 1,
    source_id: nativeInspection.source_id,
    source_label: 'Selected CPAP source',
    importer_id: 'resmed',
    status,
    phase: status === 'blocked' ? 'awaiting_session_importer' : 'finished',
    created_at_ms: 1,
    updated_at_ms: 1,
    ...(status === 'cancelled' ? { finished_at_ms: 2 } : {}),
    counts: {
      sessions_created: 0,
      sessions_updated: 0,
      events_written: 0,
      waveform_chunks_written: 0,
    },
    can_cancel: status === 'blocked',
    ...(status === 'blocked' ? { unavailable_reason: 'session_importer_not_available' } : {}),
  };
}

function makeNativeClient(overrides: Partial<OpapClient> = {}): OpapClient {
  const job = blockedJob();
  return {
    runtime: 'tauri',
    bootstrap: vi.fn(async () => nativeBootstrap()),
    listProfiles: vi.fn(async () => nativeBootstrap().profiles),
    createProfile: vi.fn(async () => nativeBootstrap().profiles[0]),
    selectNativeSource: vi.fn(async () => nativeInspection),
    prepareImportJob: vi.fn(async () => ({ job, created: true })),
    listImportJobs: vi.fn(async () => [job]),
    getImportJob: vi.fn(async () => job),
    cancelImportJob: vi.fn(async () => blockedJob('cancelled')),
    ...overrides,
  };
}

function renderImport(client: OpapClient) {
  return render(
    <MantineProvider theme={theme}>
      <MemoryRouter>
        <OpapClientProvider client={client}>
          <ImportPage />
        </OpapClientProvider>
      </MemoryRouter>
    </MantineProvider>,
  );
}

describe('Import workflow', () => {
  it('keeps the browser workflow visibly fabricated and records only a blocked, cancellable demo job', async () => {
    const user = userEvent.setup();
    renderImport(createDemoOpapClient());

    expect(await screen.findByText('Fabricated browser demonstration')).toBeInTheDocument();
    expect(screen.getByText(/No folder picker, CPAP card, or local storage is used/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /Inspect the fabricated demo source/i }));
    const reviewHeading = await screen.findByRole('heading', { name: 'Review selected source' });
    expect(reviewHeading).toHaveFocus();
    expect(screen.getByText('Supported source recognized')).toBeInTheDocument();
    expect(screen.getAllByText(/fabricated/i).length).toBeGreaterThan(0);
    expect(screen.getByText(/No therapy session has been imported/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Prepare blocked import job' }));
    const blockedHeading = await screen.findByRole('heading', { name: 'Import job prepared and blocked' });
    expect(blockedHeading).toHaveFocus();
    expect(screen.getByText(/No therapy sessions were imported/i)).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: /Built-in demo source/i })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Cancel blocked job' }));
    const cancelledHeading = await screen.findByRole('heading', { name: 'Import job cancelled' });
    expect(cancelledHeading).toHaveFocus();
    expect(screen.queryByRole('button', { name: 'Cancel blocked job' })).not.toBeInTheDocument();
  });

  it('uses the native client without rendering an opaque handle, path, or serial', async () => {
    const user = userEvent.setup();
    const client = makeNativeClient();
    renderImport(client);

    expect(await screen.findByRole('button', { name: /Choose a CPAP card folder/i })).toBeInTheDocument();
    expect(screen.queryByText(/fabricated browser demonstration/i)).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /Choose a CPAP card folder/i }));

    expect(await screen.findByText('Selected CPAP source')).toBeInTheDocument();
    expect(screen.getByText('AirSense 10 AutoSet')).toBeInTheDocument();
    expect(screen.queryByText(nativeInspection.source_id)).not.toBeInTheDocument();
    expect(screen.queryByText('9876')).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent('/Users/');

    await user.click(screen.getByRole('button', { name: 'Prepare blocked import job' }));
    await screen.findByText('Import job prepared and blocked');
    expect(client.prepareImportJob).toHaveBeenCalledWith(expect.objectContaining({
      profile_id: 7,
      source_id: nativeInspection.source_id,
    }));
    expect(client.listImportJobs).toHaveBeenCalledWith(7);
  });

  it('treats native picker cancellation as a harmless no-op', async () => {
    const user = userEvent.setup();
    const prepareImportJob = vi.fn();
    renderImport(makeNativeClient({
      selectNativeSource: vi.fn(async () => null),
      prepareImportJob,
    }));

    await user.click(await screen.findByRole('button', { name: /Choose a CPAP card folder/i }));
    expect(await screen.findByText('No folder selected')).toBeInTheDocument();
    expect(screen.getByText(/Nothing was inspected or saved/i)).toBeInTheDocument();
    expect(prepareImportJob).not.toHaveBeenCalled();
  });

  it('redacts an unexpected native error and allows the source selection to be retried', async () => {
    const user = userEvent.setup();
    const selectNativeSource = vi.fn()
      .mockRejectedValueOnce(new Error('cannot read /Users/alice/private-card'))
      .mockResolvedValueOnce(nativeInspection);
    renderImport(makeNativeClient({ selectNativeSource }));

    await user.click(await screen.findByRole('button', { name: /Choose a CPAP card folder/i }));
    expect(await screen.findByText('This action did not finish')).toBeInTheDocument();
    expect(screen.getByText('OPAP could not inspect the selected source.')).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent('/Users/alice');

    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByText('Supported source recognized')).toBeInTheDocument();
    expect(selectNativeSource).toHaveBeenCalledTimes(2);
  });

  it('shows an unsupported source without offering job preparation', async () => {
    const user = userEvent.setup();
    renderImport(makeNativeClient({
      selectNativeSource: vi.fn(async () => ({
        ...nativeInspection,
        recognized: false,
        importer_id: undefined,
        device: undefined,
        source_label: 'Selected source',
      })),
    }));

    await user.click(await screen.findByRole('button', { name: /Choose a CPAP card folder/i }));
    expect(await screen.findByText('This source is not supported yet')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Prepare blocked import job' })).not.toBeInTheDocument();
    expect(screen.getByText(/Nothing was written/i)).toBeInTheDocument();
  });

  it('keeps job controls disabled until the initial history request settles', async () => {
    const user = userEvent.setup();
    let resolveJobs: (jobs: ImportJobDto[]) => void = () => undefined;
    const jobsPending = new Promise<ImportJobDto[]>((resolve) => {
      resolveJobs = resolve;
    });
    const client = makeNativeClient({ listImportJobs: vi.fn(() => jobsPending) });
    renderImport(client);

    await user.click(await screen.findByRole('button', { name: /Choose a CPAP card folder/i }));
    await user.click(await screen.findByRole('button', { name: 'Prepare blocked import job' }));
    await screen.findByRole('heading', { name: 'Import job prepared and blocked' });

    expect(screen.getByRole('button', { name: 'Cancel blocked job' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Refresh jobs' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Inspect another source' })).toBeDisabled();

    resolveJobs([blockedJob()]);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Cancel blocked job' })).toBeEnabled());
  });

  it('does not offer a picker when bootstrap reports source inspection unavailable', async () => {
    const bootstrap = nativeBootstrap();
    bootstrap.capabilities.source_inspection = false;
    renderImport(makeNativeClient({ bootstrap: vi.fn(async () => bootstrap) }));

    expect(await screen.findByText('Source inspection unavailable')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Choose a CPAP card folder/i })).not.toBeInTheDocument();
    expect(screen.getByText(/No folder can be selected or read/i)).toBeInTheDocument();
  });
});
