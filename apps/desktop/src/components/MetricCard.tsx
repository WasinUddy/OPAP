import { Group, Paper, Stack, Text, Tooltip } from '@mantine/core';
import { ArrowDownRight, ArrowUpRight, Minus, Info } from 'lucide-react';
import type { Metric } from '../data/mockData';

const trendIcon = {
  up: ArrowUpRight,
  down: ArrowDownRight,
  steady: Minus,
};

export function MetricCard({ metric }: { metric: Metric }) {
  const TrendIcon = trendIcon[metric.trendDirection];
  const beneficial =
    (metric.label === 'Usage' && metric.trendDirection === 'up') ||
    (metric.label !== 'Usage' && metric.trendDirection === 'down');

  return (
    <Paper withBorder p="lg" className="metric-card">
      <Stack gap={10}>
        <Group justify="space-between" align="center" wrap="nowrap">
          <Text size="sm" fw={650} c="gray.7">
            {metric.label}
          </Text>
          <Tooltip label={metric.hint} withArrow>
            <button className="icon-quiet" aria-label={`About ${metric.label}`}>
              <Info size={15} />
            </button>
          </Tooltip>
        </Group>
        <div>
          <Text className="metric-value">{metric.value}</Text>
          <Text size="xs" c="dimmed" mt={2}>
            {metric.unit}
          </Text>
        </div>
        <Group gap={6} wrap="nowrap" className={beneficial ? 'trend-positive' : 'trend-neutral'}>
          <TrendIcon size={14} aria-hidden />
          <Text size="xs" fw={600}>
            {metric.trend}
          </Text>
        </Group>
      </Stack>
    </Paper>
  );
}
