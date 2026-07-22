import { MantineProvider } from '@mantine/core';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { ImportPage } from './ImportPage';
import { theme } from '../theme';

describe('Import workflow', () => {
  it('detects a supported device and reviews sessions before import', async () => {
    const user = userEvent.setup();
    render(
      <MantineProvider theme={theme}>
        <MemoryRouter>
          <ImportPage />
        </MemoryRouter>
      </MantineProvider>,
    );

    expect(screen.getByText(/No folder is selected, no CPAP card is read/i)).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /Run the simulated card detector/i }));
    expect(screen.getByText('Demo ResMed AirSense 10 result')).toBeInTheDocument();
    expect(screen.getByText('AirSense 10 AutoSet')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Review sample sessions' }));
    expect(screen.getByText('Review 28 sample sessions')).toBeInTheDocument();
    expect(screen.getByText('The preview will simulate 27 additions and one duplicate skip.')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '1 import note' }));
    await waitFor(() => {
      expect(screen.getByText(/demonstrate idempotent behavior/i)).toBeVisible();
    });
  });
});
