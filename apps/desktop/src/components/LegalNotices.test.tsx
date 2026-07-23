import { MantineProvider } from '@mantine/core';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { LegalNotices } from './LegalNotices';
import { theme } from '../theme';

describe('LegalNotices', () => {
  it('shows the modified-work, conveyance, warranty, and exact upstream notices', () => {
    render(
      <MantineProvider theme={theme}>
        <LegalNotices />
      </MantineProvider>,
    );

    const notice = screen.getByRole('region', { name: 'OPAP legal notice' });
    expect(notice).toHaveTextContent('modified software based in part on OSCAR and SleepyHead');
    expect(notice).toHaveTextContent('You may convey and modify it under GPLv3');
    expect(notice).toHaveTextContent('There is no warranty for OPAP');
    expect(notice).toHaveTextContent('Copyright © 2011–2018 Mark Watkins');
    expect(notice).toHaveTextContent('Copyright © 2019–2025 The OSCAR Team');
  });

  it('opens the complete checked-in GPL text without navigating to the web', async () => {
    const user = userEvent.setup();
    render(
      <MantineProvider theme={theme}>
        <LegalNotices />
      </MantineProvider>,
    );

    await user.click(screen.getByRole('button', { name: 'Read GPLv3 license offline' }));

    const dialog = await screen.findByRole('dialog', {
      name: 'GNU General Public License, version 3',
    });
    expect(dialog).toHaveTextContent('No internet connection is required');
    expect(dialog).toHaveTextContent('GNU GENERAL PUBLIC LICENSE');
    expect(dialog).toHaveTextContent('END OF TERMS AND CONDITIONS');
    expect(dialog).toHaveTextContent('NO WARRANTY');
    expect(dialog.querySelector('a')).toBeNull();
  });
});
