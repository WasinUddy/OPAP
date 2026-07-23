import {
  Box,
  Button,
  Divider,
  Group,
  Modal,
  Paper,
  ScrollArea,
  Stack,
  Text,
} from '@mantine/core';
import { FileText, Scale } from 'lucide-react';
import { useState } from 'react';
import gplV3Text from 'virtual:opap-copying';

const copyrightNotices = [
  'Copyright © 2011–2018 Mark Watkins',
  'Copyright © 2019–2025 The OSCAR Team',
  'Copyright © 2026 OPAP contributors',
] as const;

export function LegalNotices() {
  const [licenseOpened, setLicenseOpened] = useState(false);

  return (
    <>
      <Paper withBorder p="md" bg="gray.0" role="region" aria-label="OPAP legal notice">
        <Group gap="sm" align="flex-start" wrap="nowrap">
          <Scale size={18} color="#2589d6" aria-hidden />
          <Stack gap={8}>
            <div>
              <Text size="sm" fw={680}>Free software and source rights</Text>
              <Text size="xs" c="dimmed" mt={3} lh={1.55}>
                OPAP is modified software based in part on OSCAR and SleepyHead. You may convey and
                modify it under GPLv3. There is no warranty for OPAP, to the extent permitted by
                applicable law.
              </Text>
            </div>
            <Stack gap={2} aria-label="Copyright notices">
              {copyrightNotices.map((notice) => (
                <Text key={notice} size="xs" c="gray.7">{notice}</Text>
              ))}
            </Stack>
            <Button
              variant="light"
              size="compact-sm"
              leftSection={<FileText size={15} />}
              onClick={() => setLicenseOpened(true)}
            >
              Read GPLv3 license offline
            </Button>
          </Stack>
        </Group>
      </Paper>

      <Modal
        opened={licenseOpened}
        onClose={() => setLicenseOpened(false)}
        title="GNU General Public License, version 3"
        size="xl"
        centered
      >
        <Text size="sm" c="dimmed">
          This is the complete local copy bundled from OPAP&apos;s canonical COPYING file. No
          internet connection is required.
        </Text>
        <Divider my="md" />
        <ScrollArea h="min(62vh, 620px)" type="auto" offsetScrollbars>
          <Box
            component="pre"
            m={0}
            pr="md"
            fz={12}
            lh={1.55}
            ff="monospace"
            style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}
          >
            {gplV3Text}
          </Box>
        </ScrollArea>
      </Modal>
    </>
  );
}
