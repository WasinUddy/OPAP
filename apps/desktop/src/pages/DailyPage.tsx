import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Divider,
  Group,
  Paper,
  Stack,
  Text,
  Title,
} from '@mantine/core';
import { Calendar, ChevronLeft, ChevronRight, Download, MoreHorizontal } from 'lucide-react';
import { useState } from 'react';
import { NightGraph } from '../components/DataVisuals';
import { MetricCard } from '../components/MetricCard';
import { dailyMetrics, eventFlags } from '../data/mockData';

export function DailyPage() {
  const [selectedEvent, setSelectedEvent] = useState(eventFlags[3]);

  return (
    <Stack gap="lg">
      <Group justify="space-between" align="center" wrap="wrap" gap="md" className="daily-toolbar">
        <Group gap={6} wrap="nowrap">
          <ActionIcon variant="default" size="lg" aria-label="Previous night (preview only)" disabled><ChevronLeft size={18} /></ActionIcon>
          <Button variant="default" color="gray" leftSection={<Calendar size={16} />} disabled>
            Sample: 21 July 2026
          </Button>
          <ActionIcon variant="default" size="lg" aria-label="Next night (preview only)" disabled><ChevronRight size={18} /></ActionIcon>
          <Badge variant="light" color="yellow.8" visibleFrom="sm">Sample night</Badge>
        </Group>
        <Group gap={8}>
          <Button variant="default" color="gray" size="sm" leftSection={<Download size={15} />} disabled>Export · preview</Button>
          <ActionIcon variant="default" size="lg" aria-label="Daily view options (preview only)" disabled><MoreHorizontal size={18} /></ActionIcon>
        </Group>
      </Group>

      <Title order={1} className="mobile-page-title">Sample night: Tuesday, 21 July</Title>

      <section aria-label="Fabricated nightly therapy summary" className="metric-grid">
        {dailyMetrics.map((metric) => <MetricCard key={metric.label} metric={metric} />)}
      </section>

      <Paper withBorder className="analysis-panel">
        <Group justify="space-between" align="flex-start" p={{ base: 'md', sm: 'lg' }}>
          <div>
            <Group gap={9}>
              <Text fw={680}>Night timeline</Text>
              <Badge variant="light" color="yellow.8">1 sample session</Badge>
            </Group>
            <Text size="sm" c="dimmed" mt={3}>22:48 – 06:30 · 7 hours 42 minutes</Text>
          </div>
          <Group gap="md" visibleFrom="md">
            {[
              ['OA', 'Obstructive', '#ce526c'],
              ['CA', 'Clear airway', '#6f66c7'],
              ['H', 'Hypopnea', '#d27837'],
            ].map(([code, label, color]) => (
              <Group key={code} gap={6} wrap="nowrap">
                <span className="legend-dot" style={{ backgroundColor: color }} aria-hidden />
                <Text size="xs" c="dimmed"><strong>{code}</strong> {label}</Text>
              </Group>
            ))}
          </Group>
        </Group>
        <Divider />
        <Box p={{ base: 'md', sm: 'lg' }}>
          <div className="timeline-scroll" tabIndex={0} aria-label="Event timeline, horizontally scrollable">
            <div className="timeline-content">
              <div className="event-track" role="group" aria-labelledby="sample-events-label">
                <span id="sample-events-label" className="sr-only">Five fabricated respiratory events across the sample night</span>
                <span className="session-bar" aria-hidden />
                <ol className="event-list" aria-label="Sample respiratory events">
                  {eventFlags.map((event, index) => (
                    <li
                      key={`${event.type}-${event.left}`}
                      className="event-item"
                      style={{ left: `${event.left}%`, width: `${event.width}%` }}
                    >
                      <button
                        type="button"
                        className="event-flag"
                        style={{ backgroundColor: event.color }}
                        aria-label={`${event.label} sample event ${index + 1} at ${event.time}`}
                        aria-pressed={selectedEvent === event}
                        title={`${event.label} at ${event.time}`}
                        onClick={() => setSelectedEvent(event)}
                      />
                    </li>
                  ))}
                </ol>
                <span className="timeline-cursor" aria-hidden />
              </div>
              <div className="timeline-axis" aria-hidden>
                {['23:00', '00:00', '01:00', '02:00', '03:00', '04:00', '05:00', '06:00'].map((time) => <span key={time}>{time}</span>)}
              </div>

              <Stack gap={0} mt="md" className="waveform-stack">
                <NightGraph label="Pressure" detail="cmH₂O · 5–12" color="#2589d6" path="pressure" range="5.2 to 11.4 centimeters of water" />
                <NightGraph label="Flow rate" detail="L/min · −60–60" color="#15927c" path="flow" range="breathing flow from minus 55 to 58 liters per minute" />
                <NightGraph label="Leak rate" detail="L/min · 0–24" color="#c77700" path="leak" range="0 to 8.1 liters per minute" />
              </Stack>
            </div>
          </div>
        </Box>
        <Divider />
        <Group justify="space-between" p="md">
          <Text size="xs" c="dimmed">Static chart preview · Zoom and pan controls are not connected yet</Text>
          <Group gap={6}>
            <span className="cursor-swatch" aria-hidden />
            <Text size="xs" fw={600}>{selectedEvent.time} · {selectedEvent.label} · sample event</Text>
          </Group>
        </Group>
      </Paper>
    </Stack>
  );
}
