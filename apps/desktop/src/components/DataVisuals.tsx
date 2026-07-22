import { Box, Group, SegmentedControl, Stack, Text } from '@mantine/core';
import { useMemo, useState } from 'react';
import { trendValues, waveformPaths } from '../data/mockData';

function pointsFor(values: number[], min: number, max: number) {
  return values
    .map((value, index) => {
      const x = 54 + index * (718 / (values.length - 1));
      const y = 218 - ((value - min) / (max - min)) * 164;
      return `${x},${y}`;
    })
    .join(' ');
}

export function TherapyTrendChart({ periodLabel }: { periodLabel: string }) {
  const [series, setSeries] = useState<'ahi' | 'usage'>('ahi');
  const values = useMemo(
    () => trendValues.map((point) => (series === 'ahi' ? point.ahi : point.usage)),
    [series],
  );
  const min = series === 'ahi' ? 0 : 5;
  const max = series === 'ahi' ? 5 : 9;
  const unit = series === 'ahi' ? 'events/hour' : 'hours';

  return (
    <Stack gap="md">
      <Group justify="space-between" align="flex-start">
        <div>
          <Text fw={680}>Sample therapy trend</Text>
          <Text size="sm" c="dimmed" mt={3}>
            Fabricated {series === 'ahi' ? 'AHI' : 'therapy usage'} points · {periodLabel}
          </Text>
        </div>
        <SegmentedControl
          size="xs"
          value={series}
          onChange={(value) => setSeries(value as 'ahi' | 'usage')}
          aria-label="Trend metric"
          data={[
            { label: 'AHI', value: 'ahi' },
            { label: 'Usage', value: 'usage' },
          ]}
        />
      </Group>
      <figure className="chart-figure">
        <svg
          className="trend-chart"
          viewBox="0 0 820 270"
          role="img"
          aria-labelledby="trend-title trend-description"
        >
          <title id="trend-title">Sample {series === 'ahi' ? 'AHI' : 'therapy usage'} trend</title>
          <desc id="trend-description">
            Fabricated values for {periodLabel.toLowerCase()}, ending at {values.at(-1)} {unit}.
          </desc>
          {[54, 95, 136, 177, 218].map((y, index) => (
            <g key={y}>
              <line x1="54" y1={y} x2="772" y2={y} className="chart-gridline" />
              <text x="42" y={y + 4} textAnchor="end" className="chart-axis-label">
                {(max - index * ((max - min) / 4)).toFixed(series === 'ahi' ? 1 : 0)}
              </text>
            </g>
          ))}
          <polyline points={pointsFor(values, min, max)} className="chart-line-halo" />
          <polyline points={pointsFor(values, min, max)} className="chart-line" />
          {values.map((value, index) => {
            const x = 54 + index * (718 / (values.length - 1));
            const y = 218 - ((value - min) / (max - min)) * 164;
            return (
              <g key={trendValues[index].label}>
                <circle cx={x} cy={y} r="8" className="chart-point-hit" />
                <circle cx={x} cy={y} r="4" className="chart-point" />
                <text x={x} y="246" textAnchor="middle" className="chart-axis-label">
                  {trendValues[index].label.replace(' Jul', '')}
                </text>
              </g>
            );
          })}
          <text x="54" y="264" className="chart-axis-caption">July</text>
        </svg>
        <figcaption className="sr-only">
          {trendValues.map((point) => `${point.label}: ${series === 'ahi' ? point.ahi : point.usage} ${unit}`).join('; ')}
        </figcaption>
      </figure>
    </Stack>
  );
}

type NightGraphProps = {
  label: string;
  detail: string;
  color: string;
  path: keyof typeof waveformPaths;
  range: string;
};

export function NightGraph({ label, detail, color, path, range }: NightGraphProps) {
  return (
    <div className="night-graph-row">
      <div className="night-graph-label">
        <Text size="sm" fw={650}>{label}</Text>
        <Text size="xs" c="dimmed" mt={2}>{detail}</Text>
      </div>
      <Box className="night-graph-canvas">
        <svg
          viewBox="0 0 900 118"
          preserveAspectRatio="none"
          role="img"
          aria-label={`${label} waveform. ${range}`}
        >
          {[24, 58, 92].map((y) => <line key={y} x1="0" x2="900" y1={y} y2={y} className="wave-grid" />)}
          {[112, 225, 337, 450, 562, 675, 787].map((x) => <line key={x} x1={x} x2={x} y1="0" y2="118" className="wave-grid wave-grid-vertical" />)}
          <path d={waveformPaths[path]} fill="none" stroke={color} strokeWidth="2.2" vectorEffect="non-scaling-stroke" />
          <line x1="612" x2="612" y1="0" y2="118" className="wave-cursor" />
        </svg>
      </Box>
    </div>
  );
}
