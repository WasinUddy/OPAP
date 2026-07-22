import { Alert, Box, Button, Paper, Skeleton, Stack, Text, Title } from '@mantine/core';
import { AlertCircle, FolderInput, RefreshCw } from 'lucide-react';
import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import sleepingBreath from '../assets/sleeping-breath.svg';

export type ContentStatus = 'ready' | 'loading' | 'empty' | 'error';

type ContentStateProps = {
  status: ContentStatus;
  children: ReactNode;
  onRetry?: () => void;
};

export function ContentState({ status, children, onRetry }: ContentStateProps) {
  if (status === 'loading') {
    return (
      <Stack gap="md" role="status" aria-label="Loading therapy data">
        <Skeleton height={36} width="38%" radius="md" />
        <div className="metric-grid">
          {[0, 1, 2, 3].map((item) => (
            <Skeleton key={item} height={150} radius="lg" />
          ))}
        </div>
        <Skeleton height={340} radius="lg" />
        <span className="sr-only">Loading therapy data…</span>
      </Stack>
    );
  }

  if (status === 'empty') {
    return (
      <Paper withBorder p={{ base: 'xl', sm: 48 }} className="centered-state">
        <Box component="img" src={sleepingBreath} alt="" aria-hidden className="empty-illustration" />
        <Title order={2} fz={22} mt="sm">
          Your first clear night starts here
        </Title>
        <Text c="dimmed" maw={480} ta="center" mt={8}>
          Open the simulated import to explore how nightly details and event timelines will work. Real card reading is not connected in this preview.
        </Text>
        <Button mt="xl" leftSection={<FolderInput size={17} />} component={Link} to="/import">
          Open demo import
        </Button>
      </Paper>
    );
  }

  if (status === 'error') {
    return (
      <Alert
        variant="light"
        color="red"
        radius="lg"
        icon={<AlertCircle size={19} />}
        title="We couldn’t load this therapy data"
      >
        <Text size="sm" mb="md">
          This sample error demonstrates recovery messaging. Try loading the preview again or open Settings to inspect planned controls.
        </Text>
        <Button
          variant="light"
          color="red"
          size="xs"
          leftSection={<RefreshCw size={14} />}
          onClick={onRetry}
        >
          Try again
        </Button>
      </Alert>
    );
  }

  return children;
}
