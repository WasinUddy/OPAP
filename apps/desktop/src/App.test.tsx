import { MantineProvider } from '@mantine/core';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { App } from './App';
import { theme } from './theme';

function renderApp(path = '/') {
  return render(
    <MantineProvider theme={theme}>
      <MemoryRouter initialEntries={[path]}>
        <App />
      </MemoryRouter>
    </MantineProvider>,
  );
}

describe('OPAP desktop shell', () => {
  it('persistently discloses fabricated sample data and describes the chart', async () => {
    renderApp();

    const demoNotice = await screen.findByRole('note', { name: 'Demo data notice' });
    expect(demoNotice).toHaveTextContent('Every clinical value and import result shown is fabricated');
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

    expect(await screen.findByText(/SleepyHead, copyright © 2011–2018 Mark Watkins/)).toHaveTextContent(
      'OSCAR, copyright © 2019–2026 The OSCAR Team',
    );
    expect(screen.getByText(/GNU General Public License, version 3 \(GPLv3\)/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Preview source repository · revision unavailable/i })).toHaveAttribute(
      'href',
      'https://github.com/WasinUddy/OPAP',
    );
  });
});
