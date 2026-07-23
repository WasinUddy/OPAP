import { MantineProvider } from '@mantine/core';
import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { App } from './App';
import type { AppBootstrap, OpapClient } from './client';
import { createDemoOpapClient } from './client';
import { theme } from './theme';

function renderApp(path = '/', client?: OpapClient) {
  return render(
    <MantineProvider theme={theme}>
      <MemoryRouter initialEntries={[path]}>
        <App client={client} />
      </MemoryRouter>
    </MantineProvider>,
  );
}

describe('OPAP desktop shell', () => {
  it('persistently discloses fabricated sample data and describes the chart', async () => {
    renderApp();

    const demoNotice = await screen.findByRole('note', { name: 'Demo data notice' });
    expect(demoNotice).toHaveTextContent('Every clinical value, source, and import job shown is fabricated');
    expect(demoNotice).toHaveTextContent('No CPAP card or local file is read');
    expect((await screen.findAllByText('7h 42m')).length).toBeGreaterThan(0);
    expect(screen.getByRole('img', { name: /Sample AHI trend/i })).toBeInTheDocument();
    expect(screen.getByText('Sample usage consistency')).toBeInTheDocument();
  });

  it('navigates between overview and daily analysis', async () => {
    const user = userEvent.setup();
    renderApp();

    await user.click((await screen.findAllByRole('link', { name: 'Daily' }))[0]);

    expect(await screen.findByText('Night timeline')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: /Pressure waveform/i })).toBeInTheDocument();
    expect(screen.getByRole('group', { name: /fabricated respiratory events/i })).toBeInTheDocument();
    expect(screen.getByRole('list', { name: 'Sample respiratory events' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Hypopnea sample event 4/i })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: /Export · preview/i })).toBeDisabled();
    expect(screen.getAllByRole('link', { name: 'Daily' })[0]).toHaveAttribute('aria-current', 'page');
  });

  it('provides useful empty and error recovery states', async () => {
    const { unmount } = renderApp('/?state=empty');
    expect(await screen.findByText('Your first clear night starts here')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open demo import' })).toBeInTheDocument();
    unmount();

    renderApp('/?state=error');
    expect(await screen.findByRole('alert')).toHaveTextContent('We couldn’t load this therapy data');
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });

  it('shows exact project attribution and a labelled preview source link', async () => {
    renderApp('/settings');

    expect(await screen.findByText('Copyright © 2011–2018 Mark Watkins')).toBeInTheDocument();
    expect(screen.getByText('Copyright © 2019–2025 The OSCAR Team')).toBeInTheDocument();
    expect(screen.getByText(/GNU General Public License, version 3 \(GPLv3\)/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Read GPLv3 license offline' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Preview source repository · revision unavailable/i })).toHaveAttribute(
      'href',
      'https://github.com/WasinUddy/OPAP',
    );
  });

  it('keeps the fabricated browser disclosure visible while capabilities load', async () => {
    const demo = createDemoOpapClient();
    let resolveBootstrap: (value: AppBootstrap) => void = () => undefined;
    const pendingBootstrap = new Promise<AppBootstrap>((resolve) => {
      resolveBootstrap = resolve;
    });
    renderApp('/', { ...demo, bootstrap: () => pendingBootstrap });

    expect(screen.getByRole('note', { name: 'Demo data notice' })).toHaveTextContent(
      'Every clinical value, source, and import job shown is fabricated',
    );
    expect(screen.getByText('Browser demo')).toBeInTheDocument();

    await act(async () => {
      resolveBootstrap(await demo.bootstrap());
    });
  });

  it('surfaces a native bootstrap failure without falling back or exposing error details', async () => {
    const unavailable = vi.fn(async () => {
      throw new Error('failed at /Users/alice/private/opap.db');
    });
    const client: OpapClient = {
      runtime: 'tauri',
      bootstrap: unavailable,
      listProfiles: unavailable,
      createProfile: unavailable,
      selectNativeSource: unavailable,
      prepareImportJob: unavailable,
      listImportJobs: unavailable,
      getImportJob: unavailable,
      cancelImportJob: unavailable,
    };
    renderApp('/import', client);

    const runtimeAlert = await screen.findByRole('alert', { name: 'Runtime status' });
    expect(runtimeAlert).toHaveTextContent('The native OPAP service returned an unexpected error.');
    expect(document.body).not.toHaveTextContent('/Users/alice');
    expect(screen.queryByRole('note', { name: 'Demo data notice' })).not.toBeInTheDocument();
  });
});
