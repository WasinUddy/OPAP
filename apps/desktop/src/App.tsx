import { Center, Loader, Text } from '@mantine/core';
import { lazy, Suspense } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';
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

export function App() {
  return (
    <Suspense fallback={<RouteLoading />}>
      <Routes>
        <Route element={<AppFrame />}>
          <Route index element={<OverviewPage />} />
          <Route path="daily" element={<DailyPage />} />
          <Route path="import" element={<ImportPage />} />
          <Route path="settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </Suspense>
  );
}
