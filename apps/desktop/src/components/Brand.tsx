import { Box, Group, Text } from '@mantine/core';
import sleepingBreath from '../assets/sleeping-breath.svg';

type BrandProps = {
  compact?: boolean;
};

export function Brand({ compact = false }: BrandProps) {
  return (
    <Group gap={10} wrap="nowrap" aria-label="OPAP home">
      <Box component="img" src={sleepingBreath} alt="" aria-hidden className="brand-mark" />
      {!compact && (
        <div>
          <Text fw={720} fz="lg" lh={1.05} c="gray.9" className="brand-wordmark">
            OPAP
          </Text>
          <Text fz={10} fw={600} c="gray.6" tt="uppercase" lts="0.11em">
            Sleep clearly
          </Text>
        </div>
      )}
    </Group>
  );
}
