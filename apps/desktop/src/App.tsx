import { Center, Loader, Text } from '@mantine/core';
import { lazy, type ReactNode, Suspense } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';
import type { OpapClient } from './client';
import { OpapClientProvider } from './client';
import { AppFrame } from './components/AppFrame';

const DailyPage = lazy(() => import('./pages/DailyPage').then((module) => ({ default: module.DailyPage })));
const ImportPage = lazy(() => import('./pages/ImportPage').then((module) => ({ default: module.ImportPage })));
const OverviewPage = lazy(() => import('./pages/OverviewPage').then((module) => ({ default: module.OverviewPage })));
const SettingsPage = lazy(() => import('./pages/SettingsPage').then((module) => ({ default: module.SettingsPage })));

function RouteLoading() {
  return (
    <Center mih={320} role="status" aria-label="Loading preview screen">
      <Loader size="sm" />
      <Text size="sm" c="dimmed" ml="sm">Loading preview…</Text>
    </Center>
  );
}

function LazyScreen({ children }: { children: ReactNode }) {
  return <Suspense fallback={<RouteLoading />}>{children}</Suspense>;
}

export function App({ client }: { client?: OpapClient }) {
  return (
    <OpapClientProvider client={client}>
      <Routes>
        <Route element={<AppFrame />}>
          <Route index element={<LazyScreen><OverviewPage /></LazyScreen>} />
          <Route path="daily" element={<LazyScreen><DailyPage /></LazyScreen>} />
          <Route path="import" element={<LazyScreen><ImportPage /></LazyScreen>} />
          <Route path="settings" element={<LazyScreen><SettingsPage /></LazyScreen>} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </OpapClientProvider>
  );
}
