import {
  Anchor,
  Avatar,
  Badge,
  Box,
  Button,
  Divider,
  Group,
  Paper,
  SegmentedControl,
  Select,
  Stack,
  Switch,
  Text,
  TextInput,
  ThemeIcon,
  Title,
} from '@mantine/core';
import { Code2, Database, ExternalLink, FolderOpen, HeartPulse, ShieldCheck, UserRound } from 'lucide-react';
import sleepingBreath from '../assets/sleeping-breath.svg';
import { buildInfo } from '../buildInfo';
import { LegalNotices } from '../components/LegalNotices';

export function SettingsPage() {
  return (
    <Stack gap="lg" maw={840} mx="auto" w="100%">
      <Box>
        <Title order={1} className="mobile-page-title">Settings & About</Title>
        <Text size="sm" c="dimmed">Preview the planned local profile, display, and privacy preferences. Settings are not saved yet.</Text>
      </Box>

      <Paper withBorder p={{ base: 'md', sm: 'xl' }}>
        <Group gap="sm" mb="xl">
          <ThemeIcon variant="light" size="lg"><UserRound size={18} /></ThemeIcon>
          <div><Text fw={680}>Profile</Text><Text size="xs" c="dimmed">Disabled sample controls for the planned local profile</Text></div>
        </Group>
        <Group align="flex-end" wrap="wrap">
          <Avatar size={48} radius="xl" color="opapBlue">DP</Avatar>
          <TextInput label="Demo display name · preview" value="Demo sleeper" flex={1} miw={220} disabled />
          <Select label="Preferred units · preview" defaultValue="metric" data={[{ value: 'metric', label: 'Metric' }, { value: 'imperial', label: 'Imperial' }]} allowDeselect={false} w={190} disabled />
        </Group>
      </Paper>

      <Paper withBorder p={{ base: 'md', sm: 'xl' }}>
        <Text fw={680}>Appearance</Text>
        <Text size="xs" c="dimmed" mt={3} mb="lg">Choose how OPAP looks on this device</Text>
        <Group justify="space-between" align="center" gap="md" wrap="wrap">
          <div><Text size="sm" fw={620}>Color theme</Text><Text size="xs" c="dimmed">A calm light theme is recommended for clinical charts.</Text></div>
          <SegmentedControl data={['Light', 'System']} defaultValue="Light" aria-label="Color theme (preview only)" disabled />
        </Group>
        <Divider my="lg" />
        <Group justify="space-between" align="center" gap="md" wrap="wrap">
          <div><Text size="sm" fw={620}>Graph density</Text><Text size="xs" c="dimmed">Controls vertical spacing in nightly analysis.</Text></div>
          <SegmentedControl data={['Comfortable', 'Compact']} defaultValue="Comfortable" aria-label="Graph density (preview only)" disabled />
        </Group>
      </Paper>

      <Paper withBorder p={{ base: 'md', sm: 'xl' }}>
        <Group gap="sm" mb="xl">
          <ThemeIcon variant="light" color="opapTeal" size="lg"><Database size={18} /></ThemeIcon>
          <div><Text fw={680}>Data & privacy</Text><Text size="xs" c="dimmed">Planned local-first controls shown for interface review</Text></div>
        </Group>
        <TextInput
          label="Planned local data location · preview"
          value="~/Library/Application Support/OPAP/profiles/demo"
          readOnly
          disabled
          rightSection={<FolderOpen size={16} />}
        />
        <Divider my="lg" />
        <Stack gap="lg">
          <Switch
            checked
            disabled
            label="Check for application updates · preview"
            description="Disabled in this interface preview. No network request is made."
          />
          <Switch
            checked={false}
            disabled
            label="Share anonymous diagnostics · preview"
            description="Disabled and off. The preview does not send diagnostic or therapy data."
          />
        </Stack>
        <Group mt="xl"><Button variant="default" color="gray" disabled>Open data folder · preview</Button><Button variant="subtle" color="red" disabled>Delete local profile · preview</Button></Group>
      </Paper>

      <Paper withBorder p={{ base: 'md', sm: 'xl' }} className="about-card">
        <div className="about-copy">
          <Group gap="md" align="center">
            <Box component="img" src={sleepingBreath} alt="The OPAP sleeping breath mark" className="about-illustration" />
            <div>
              <Group gap={8}><Title order={2} fz={22}>OPAP</Title><Badge variant="light">{buildInfo.version}</Badge></Group>
              <Text size="sm" c="dimmed" mt={3}>Open CPAP insights, thoughtfully redesigned.</Text>
            </div>
          </Group>
          <Text size="sm" c="gray.7" lh={1.65} mt="xl">
            This interface preview contains fabricated sample data. OPAP is an independent,
            modified derivative of OSCAR and SleepyHead, distributed under the GNU General Public
            License, version 3 (GPLv3).
          </Text>
          <Box mt="lg"><LegalNotices /></Box>
          <Group mt="lg" gap="sm">
            <Button component="a" href={buildInfo.sourceHref} target="_blank" rel="noreferrer" variant="default" color="gray" leftSection={<Code2 size={16} />} rightSection={<ExternalLink size={13} />}>{buildInfo.sourceLabel}</Button>
          </Group>
        </div>
        <Divider my="xl" />
        <Group align="flex-start" gap="md" wrap="nowrap">
          <ThemeIcon color="yellow" variant="light" size="lg"><HeartPulse size={18} /></ThemeIcon>
          <div>
            <Text size="sm" fw={650}>For understanding, not diagnosis</Text>
            <Text size="xs" c="dimmed" mt={4} lh={1.55}>This preview uses fabricated values. Future device-connected releases will present recorded information, not medical advice, and should not replace guidance from a qualified clinician.</Text>
          </div>
        </Group>
        <Group gap={7} mt="lg">
          <ShieldCheck size={15} color="#15927c" />
          <Text size="xs" c="dimmed">Local-first · Open source · No account required</Text>
        </Group>
      </Paper>

      <Text size="xs" c="dimmed" ta="center">
        Need help? <Anchor href="https://github.com/WasinUddy/OPAP/issues" target="_blank">Visit the community issue tracker</Anchor>.
      </Text>
    </Stack>
  );
}
