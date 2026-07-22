import {
  Alert,
  Badge,
  Box,
  Button,
  Collapse,
  Divider,
  Group,
  List,
  Paper,
  Progress,
  Stack,
  Stepper,
  Table,
  Text,
  ThemeIcon,
  Title,
} from '@mantine/core';
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronUp,
  CircleCheck,
  FolderOpen,
  HardDrive,
  LockKeyhole,
  MoonStar,
  RefreshCw,
  Usb,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import sleepingBreath from '../assets/sleeping-breath.svg';

const reviewRows = [
  ['21 Jul 2026', '7h 42m', 'New'],
  ['20 Jul 2026', '7h 18m', 'New'],
  ['19 Jul 2026', '8h 06m', 'New'],
  ['18 Jul 2026', '6h 54m', 'Already imported'],
];

export function ImportPage() {
  const [active, setActive] = useState(0);
  const [detected, setDetected] = useState(false);
  const [progress, setProgress] = useState(0);
  const [warningsOpen, setWarningsOpen] = useState(false);
  const progressMilestone = progress >= 100 ? 100 : progress >= 75 ? 75 : progress >= 50 ? 50 : progress >= 25 ? 25 : 0;

  useEffect(() => {
    if (active !== 2 || progress >= 100) return undefined;
    const timer = window.setInterval(() => {
      setProgress((current) => {
        const next = Math.min(100, current + 13);
        if (next === 100) window.setTimeout(() => setActive(3), 160);
        return next;
      });
    }, 120);
    return () => window.clearInterval(timer);
  }, [active, progress]);

  return (
    <Stack gap="lg" maw={1000} mx="auto" w="100%">
      <Box>
        <Title order={1} className="mobile-page-title">Preview an import</Title>
        <Text size="sm" c="dimmed">
          Demo workflow only. No folder is selected, no CPAP card is read, and no therapy data is written.
        </Text>
      </Box>

      <Paper withBorder p={{ base: 'md', sm: 'xl' }}>
        <Stepper active={active} color="opapBlue" iconSize={34} mb={36}>
          <Stepper.Step label="Demo source" description="Use built-in sample" icon={<FolderOpen size={17} />} completedIcon={<Check size={17} />} />
          <Stepper.Step label="Review" description="See sample sessions" icon={<HardDrive size={17} />} completedIcon={<Check size={17} />} />
          <Stepper.Step label="Simulate" description="Preview import states" icon={<RefreshCw size={17} />} completedIcon={<Check size={17} />} />
        </Stepper>

        {active === 0 && (
          <Stack gap="lg">
            {!detected ? (
              <button className="drop-zone" onClick={() => setDetected(true)} type="button">
                <ThemeIcon variant="light" color="opapBlue" size={54} radius="xl"><Usb size={25} /></ThemeIcon>
                <div>
                  <Badge variant="light" color="yellow.8" mb="sm">Demo workflow</Badge>
                  <Text fw={680} fz="lg">Run the simulated card detector</Text>
                  <Text size="sm" c="dimmed" mt={5}>Uses a built-in fictional ResMed card. It will not open a folder picker.</Text>
                </div>
                <span className="drop-zone-button">Run simulated detection</span>
                <Text size="xs" c="dimmed">Real device reading is planned but not connected</Text>
              </button>
            ) : (
              <Stack gap="md">
                <Alert color="opapTeal" variant="light" icon={<CircleCheck size={19} />} title="Demo ResMed AirSense 10 result">
                  Simulated detection found 28 fabricated therapy sessions. No device was read.
                </Alert>
                <Paper withBorder p="lg" className="detected-device">
                  <Group justify="space-between" align="flex-start" wrap="wrap" gap="lg">
                    <Group align="flex-start" gap="md">
                      <ThemeIcon variant="light" size={44} radius="md"><HardDrive size={21} /></ThemeIcon>
                      <div>
                        <Text fw={680}>AirSense 10 AutoSet</Text>
                        <Text size="sm" c="dimmed" mt={2}>ResMed · Serial ending 4832</Text>
                      </div>
                    </Group>
                    <Badge variant="light" color="yellow.8">Sample device</Badge>
                  </Group>
                  <Divider my="lg" />
                  <div className="device-detail-grid">
                    <div><Text size="xs" c="dimmed">Available dates</Text><Text size="sm" fw={620} mt={3}>24 Jun – 21 Jul 2026</Text></div>
                    <div><Text size="xs" c="dimmed">Sessions found</Text><Text size="sm" fw={620} mt={3}>28 nights</Text></div>
                    <div><Text size="xs" c="dimmed">Source</Text><Text size="sm" fw={620} mt={3}>Built-in sample / DATALOG</Text></div>
                  </div>
                </Paper>
                <Group justify="space-between">
                  <Button variant="subtle" color="gray" onClick={() => setDetected(false)}>Reset demo</Button>
                  <Button onClick={() => setActive(1)}>Review sample sessions</Button>
                </Group>
              </Stack>
            )}
            <Group justify="center" gap={6}>
              <LockKeyhole size={14} color="#66717f" />
              <Text size="xs" c="dimmed">Simulation only · no file access or persistence occurs</Text>
            </Group>
          </Stack>
        )}

        {active === 1 && (
          <Stack gap="lg">
            <Group justify="space-between" align="flex-end" wrap="wrap">
              <div>
                <Text fw={680} fz="lg">Review 28 sample sessions</Text>
                <Text size="sm" c="dimmed" mt={3}>The preview will simulate 27 additions and one duplicate skip.</Text>
              </div>
              <Badge variant="light" color="yellow.8">Sample: 27 new · 1 existing</Badge>
            </Group>
            <Table.ScrollContainer minWidth={500}>
              <Table striped withTableBorder verticalSpacing="sm">
                <Table.Thead><Table.Tr><Table.Th>Night</Table.Th><Table.Th>Usage</Table.Th><Table.Th>Status</Table.Th></Table.Tr></Table.Thead>
                <Table.Tbody>
                  {reviewRows.map(([night, usage, status]) => (
                    <Table.Tr key={night}>
                      <Table.Td fw={620}>{night}</Table.Td>
                      <Table.Td className="numeric-cell">{usage}</Table.Td>
                      <Table.Td><Badge variant="light" color={status === 'New' ? 'opapBlue' : 'gray'}>{status}</Badge></Table.Td>
                    </Table.Tr>
                  ))}
                </Table.Tbody>
              </Table>
            </Table.ScrollContainer>
            <button className="warning-toggle" type="button" onClick={() => setWarningsOpen((open) => !open)} aria-expanded={warningsOpen}>
              <Group gap={8}><AlertTriangle size={16} color="#c77700" /><Text size="sm" fw={620}>1 import note</Text></Group>
              {warningsOpen ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
            </button>
            <Collapse expanded={warningsOpen}>
              <Alert color="yellow" variant="light">
                In this simulation, 18 July already exists and is skipped to demonstrate idempotent behavior.
              </Alert>
            </Collapse>
            <Group justify="space-between">
              <Button variant="subtle" color="gray" onClick={() => setActive(0)}>Back</Button>
              <Button onClick={() => { setProgress(8); setActive(2); }}>Simulate import of 27 sessions</Button>
            </Group>
          </Stack>
        )}

        {active === 2 && (
          <Stack align="center" gap="lg" py={36}>
            <Box component="img" src={sleepingBreath} alt="" aria-hidden className="import-illustration" />
            <div className="import-progress-copy">
              <Text fw={680} fz="lg" ta="center">Simulating an import</Text>
              <Text size="sm" c="dimmed" ta="center" mt={5}>
                {progress < 65 ? 'Previewing parser progress…' : 'Previewing summary calculations…'}
              </Text>
            </div>
            <span className="sr-only" aria-live="polite" aria-atomic="true">
              Simulated import {progressMilestone}% complete
            </span>
            <Box w="100%" maw={520}>
              <Group justify="space-between" mb={7}>
                <Text size="xs" c="dimmed">{Math.min(27, Math.round((progress / 100) * 27))} of 27 sessions</Text>
                <Text size="xs" fw={650} className="numeric-cell">{progress}%</Text>
              </Group>
              <Progress value={progress} size="md" radius="xl" animated aria-label="Simulated import progress" />
            </Box>
            <Text size="xs" c="dimmed">No files are being read and no records are being saved.</Text>
          </Stack>
        )}

        {active >= 3 && (
          <Stack align="center" gap="lg" py={30} aria-live="polite">
            <ThemeIcon color="opapTeal" variant="light" size={64} radius="xl"><Check size={29} /></ThemeIcon>
            <div>
              <Title order={2} fz={23} ta="center">Demo import complete</Title>
              <Text c="dimmed" ta="center" mt={7}>The sample workflow finished. No therapy data was added or changed.</Text>
            </div>
            <List size="sm" spacing="xs" icon={<ThemeIcon color="opapTeal" size={20} radius="xl"><Check size={12} /></ThemeIcon>}>
              <List.Item>27 fabricated sessions simulated</List.Item>
              <List.Item>4,892 fictional samples represented</List.Item>
              <List.Item>Sample nightly summaries displayed</List.Item>
            </List>
            <Group>
              <Button component={Link} to="/daily" leftSection={<MoonStar size={16} />}>Open sample night</Button>
              <Button variant="default" color="gray" onClick={() => { setActive(0); setDetected(false); setProgress(0); }}>Restart demo</Button>
            </Group>
          </Stack>
        )}
      </Paper>
    </Stack>
  );
}
