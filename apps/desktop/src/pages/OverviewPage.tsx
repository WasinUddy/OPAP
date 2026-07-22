import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Group,
  Paper,
  RingProgress,
  Select,
  Stack,
  Table,
  Text,
  Title,
} from '@mantine/core';
import { ArrowRight, CalendarRange, ChevronRight, Sparkles } from 'lucide-react';
import { useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { ContentState, type ContentStatus } from '../components/ContentState';
import { TherapyTrendChart } from '../components/DataVisuals';
import { MetricCard } from '../components/MetricCard';
import { overviewMetrics, recentNights } from '../data/mockData';

export function OverviewPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const [range, setRange] = useState('last-7');
  const rangeLabel = {
    'last-7': 'Last 7 sample nights',
    'last-30': 'Last 30 sample nights',
    'last-90': 'Last 90 sample nights',
  }[range] ?? 'Last 7 sample nights';
  const requestedState = searchParams.get('state');
  const status: ContentStatus = ['loading', 'empty', 'error'].includes(requestedState ?? '')
    ? (requestedState as ContentStatus)
    : 'ready';

  return (
    <Stack gap="lg">
      <Group justify="space-between" align="flex-end" wrap="wrap" gap="md">
        <Box>
          <Title order={1} className="mobile-page-title">OPAP interface preview</Title>
          <Group gap={7} mt={5}>
            <Sparkles size={15} color="#15927c" aria-hidden />
            <Text size="sm" c="dimmed">
              A fabricated profile demonstrates how therapy trends could look.
            </Text>
          </Group>
        </Box>
        <Select
          aria-label="Overview date range"
          value={range}
          onChange={(value) => value && setRange(value)}
          leftSection={<CalendarRange size={16} />}
          data={[
            { value: 'last-7', label: 'Last 7 sample nights' },
            { value: 'last-30', label: 'Last 30 sample nights' },
            { value: 'last-90', label: 'Last 90 sample nights' },
          ]}
          w={180}
          allowDeselect={false}
        />
      </Group>

      <ContentState status={status} onRetry={() => setSearchParams({})}>
        <Stack gap="lg">
          <section aria-label="Fabricated last night summary" className="metric-grid">
            {overviewMetrics.map((metric) => <MetricCard key={metric.label} metric={metric} />)}
          </section>

          <Paper withBorder p={{ base: 'md', sm: 'xl' }}>
            <TherapyTrendChart periodLabel={rangeLabel} />
          </Paper>

          <div className="overview-lower-grid">
            <Paper withBorder p={{ base: 'md', sm: 'xl' }}>
              <Group justify="space-between" mb="lg">
                <div>
                  <Text fw={680}>Recent sample nights</Text>
                  <Text size="sm" c="dimmed" mt={3}>Five fabricated sessions for interface evaluation</Text>
                </div>
                <Button component={Link} to="/daily" variant="subtle" size="xs" rightSection={<ArrowRight size={14} />}>
                  Open daily view
                </Button>
              </Group>
              <Table.ScrollContainer minWidth={560}>
                <Table verticalSpacing="sm" horizontalSpacing="sm" highlightOnHover>
                  <Table.Thead>
                    <Table.Tr>
                      <Table.Th>Night</Table.Th>
                      <Table.Th>Usage</Table.Th>
                      <Table.Th>AHI</Table.Th>
                      <Table.Th>Leak</Table.Th>
                      <Table.Th>Data</Table.Th>
                    </Table.Tr>
                  </Table.Thead>
                  <Table.Tbody>
                    {recentNights.map((night) => (
                      <Table.Tr key={night.date}>
                        <Table.Td>
                          <Text size="sm" fw={620}>{night.date}</Text>
                          <Text size="xs" c="dimmed">{night.day}</Text>
                        </Table.Td>
                        <Table.Td className="numeric-cell">{night.duration}</Table.Td>
                        <Table.Td className="numeric-cell">{night.ahi.toFixed(1)}</Table.Td>
                        <Table.Td className="numeric-cell">{night.leak.toFixed(1)} L/min</Table.Td>
                        <Table.Td>
                          <Group justify="flex-end" gap={4} wrap="nowrap">
                            <Badge color="gray" variant="light" size="sm">{night.recording}</Badge>
                            <ActionIcon
                              component={Link}
                              to="/daily"
                              variant="subtle"
                              color="gray"
                              size="sm"
                              aria-label={`Open ${night.date}`}
                            >
                              <ChevronRight size={15} />
                            </ActionIcon>
                          </Group>
                        </Table.Td>
                      </Table.Tr>
                    ))}
                  </Table.Tbody>
                </Table>
              </Table.ScrollContainer>
            </Paper>

            <Paper withBorder p={{ base: 'md', sm: 'xl' }} className="consistency-card">
              <div>
                <Text fw={680}>Sample usage consistency</Text>
                <Text size="sm" c="dimmed" mt={3}>Fabricated 30-night summary</Text>
              </div>
              <Box className="consistency-ring-wrap">
                <RingProgress
                  size={176}
                  thickness={12}
                  roundCaps
                  sections={[{ value: 93, color: 'opapTeal.6' }]}
                  label={
                    <Box ta="center">
                      <Text fz={32} fw={720} className="numeric-cell">93%</Text>
                      <Text size="xs" c="dimmed">nights ≥ 4h</Text>
                    </Box>
                  }
                />
              </Box>
              <Stack gap={9}>
                <Group justify="space-between">
                  <Text size="sm" c="dimmed">Used 4+ hours</Text>
                  <Text size="sm" fw={650} className="numeric-cell">28 / 30</Text>
                </Group>
                <Group justify="space-between">
                  <Text size="sm" c="dimmed">Average duration</Text>
                  <Text size="sm" fw={650} className="numeric-cell">7h 28m</Text>
                </Group>
                <Group justify="space-between">
                  <Text size="sm" c="dimmed">Current streak</Text>
                  <Badge variant="light" color="opapTeal">27 nights</Badge>
                </Group>
              </Stack>
            </Paper>
          </div>
        </Stack>
      </ContentState>
    </Stack>
  );
}
