import {
  AppShell,
  Avatar,
  Badge,
  Box,
  Button,
  Burger,
  Divider,
  Group,
  Menu,
  Stack,
  Text,
  UnstyledButton,
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { CalendarDays, ChevronDown, FlaskConical, FolderInput, LayoutDashboard, Settings } from 'lucide-react';
import type { ComponentType } from 'react';
import { Link, Outlet, useLocation } from 'react-router-dom';
import { useOpapClient } from '../client';
import { Brand } from './Brand';

type NavigationItem = {
  label: string;
  path: string;
  icon: ComponentType<{ size?: number; strokeWidth?: number }>;
};

const primaryNavigation: NavigationItem[] = [
  { label: 'Overview', path: '/', icon: LayoutDashboard },
  { label: 'Daily', path: '/daily', icon: CalendarDays },
  { label: 'Import', path: '/import', icon: FolderInput },
];

const secondaryNavigation: NavigationItem[] = [
  { label: 'Settings & About', path: '/settings', icon: Settings },
];

const pageMeta: Record<string, { eyebrow: string; title: string }> = {
  '/': { eyebrow: 'Sample therapy overview', title: 'OPAP interface preview' },
  '/daily': { eyebrow: 'Sample nightly details', title: 'Tuesday, 21 July · demo' },
  '/import': { eyebrow: 'Privacy-safe source inspection', title: 'Prepare an import' },
  '/settings': { eyebrow: 'Preferences', title: 'Settings & About' },
};

function NavigationLink({ item, onNavigate }: { item: NavigationItem; onNavigate?: () => void }) {
  const location = useLocation();
  const active = location.pathname === item.path;
  const Icon = item.icon;

  return (
    <UnstyledButton
      component={Link}
      to={item.path}
      onClick={onNavigate}
      className={`nav-item${active ? ' nav-item-active' : ''}`}
      aria-current={active ? 'page' : undefined}
    >
      <Icon size={18} strokeWidth={1.9} aria-hidden />
      <span>{item.label}</span>
    </UnstyledButton>
  );
}

export function AppFrame() {
  const [opened, { toggle, close }] = useDisclosure();
  const location = useLocation();
  const meta = pageMeta[location.pathname] ?? pageMeta['/'];
  const { capabilities, errorMessage, profiles, retryBootstrap, runtime, status } = useOpapClient();
  const profile = profiles[0];
  const profileName = profile?.display_name ?? (status === 'loading' ? 'Loading profile…' : 'No local profile');
  const profileInitials = getInitials(profile?.display_name);
  const bannerState = runtime === 'demo' ? 'demo' : status === 'error' ? 'error' : status === 'loading' ? 'loading' : runtime;
  const runtimeBadge = runtime === 'demo'
    ? { color: 'yellow.9', icon: <FlaskConical size={13} />, label: 'Browser demo', shortLabel: 'Demo' }
    : status === 'error'
    ? { color: 'red.7', icon: <FolderInput size={13} />, label: 'Service error', shortLabel: 'Error' }
    : status === 'loading'
      ? { color: 'gray.7', icon: <FolderInput size={13} />, label: 'Starting OPAP', shortLabel: 'Starting' }
      : runtime === 'tauri'
      ? { color: 'opapBlue', icon: <FolderInput size={13} />, label: 'Desktop preview', shortLabel: 'Desktop' }
      : { color: 'red.7', icon: <FolderInput size={13} />, label: 'Service unavailable', shortLabel: 'Unavailable' };

  return (
    <AppShell
      header={{ height: 72 }}
      navbar={{ width: 240, breakpoint: 'sm', collapsed: { mobile: !opened } }}
      padding={0}
    >
      <AppShell.Header className="app-header">
        <Group h="100%" px={{ base: 'md', sm: 'xl' }} justify="space-between" wrap="nowrap">
          <Group gap="sm" wrap="nowrap">
            <Burger opened={opened} onClick={toggle} hiddenFrom="sm" size="sm" aria-label="Toggle navigation" />
            <Box hiddenFrom="sm"><Brand /></Box>
            <Box visibleFrom="sm">
              <Text className="header-eyebrow">{meta.eyebrow}</Text>
              <Text fw={680} fz="lg" lh={1.25}>{meta.title}</Text>
            </Box>
          </Group>

          <Group gap={8} wrap="nowrap">
            <Badge
              variant={runtime === 'demo' ? 'outline' : 'filled'}
              color={runtimeBadge.color}
              leftSection={runtimeBadge.icon}
              visibleFrom="xs"
            >
              {runtimeBadge.label}
            </Badge>
            <Badge variant={runtime === 'demo' ? 'outline' : 'filled'} color={runtimeBadge.color} hiddenFrom="xs">
              {runtimeBadge.shortLabel}
            </Badge>
            <Menu position="bottom-end" shadow="md" width={230}>
              <Menu.Target>
                <UnstyledButton className="profile-button" aria-label="Open profile menu">
                  <Group gap={8} wrap="nowrap">
                    <Avatar size={34} radius="xl" color="opapBlue">{profileInitials}</Avatar>
                    <Box visibleFrom="xs">
                      <Text size="sm" fw={650} lh={1.15}>{profileName}</Text>
                      <Text size="xs" c="dimmed" lh={1.15}>
                        {runtime === 'demo' ? 'Fabricated browser preview' : 'Local profile'}
                      </Text>
                    </Box>
                    <ChevronDown size={14} aria-hidden />
                  </Group>
                </UnstyledButton>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Label>{runtime === 'demo' ? 'Fabricated preview profile' : 'Local profile'}</Menu.Label>
                <Menu.Item disabled leftSection={<Avatar size={22} color="opapBlue">{profileInitials}</Avatar>}>
                  {profileName}
                </Menu.Item>
                <Menu.Divider />
                <Menu.Item component={Link} to="/settings" leftSection={<Settings size={15} />}>
                  Manage profiles
                </Menu.Item>
              </Menu.Dropdown>
            </Menu>
          </Group>
        </Group>
      </AppShell.Header>

      <AppShell.Navbar className="app-navbar" p="md">
        <AppShell.Section px={6} pt={4} pb="xl">
          <Brand />
        </AppShell.Section>
        <AppShell.Section grow>
          <Stack gap={5}>
            <Text className="nav-label">Workspace</Text>
            {primaryNavigation.map((item) => (
              <NavigationLink key={item.path} item={item} onNavigate={close} />
            ))}
          </Stack>
        </AppShell.Section>
        <AppShell.Section>
          <Divider mb="md" />
          <Stack gap={5}>
            {secondaryNavigation.map((item) => (
              <NavigationLink key={item.path} item={item} onNavigate={close} />
            ))}
          </Stack>
          <Text fz={11} c="gray.6" px="sm" mt="lg">OPAP preview · v0.1.0</Text>
        </AppShell.Section>
      </AppShell.Navbar>

      <AppShell.Main component="main" className="app-main" id="main-content">
        <section
          className={`runtime-banner runtime-banner-${bannerState}`}
          role={status === 'error' ? 'alert' : 'note'}
          aria-label={runtime === 'demo' ? 'Demo data notice' : 'Runtime status'}
        >
          <Group justify="center" gap={9} wrap="wrap">
            {runtime === 'demo' ? (
              <>
                <Badge variant="outline" color="yellow.9" size="sm">Browser demo · fabricated</Badge>
                <Text size="sm" fw={620}>
                  Every clinical value, source, and import job shown is fabricated. No CPAP card or local file is read.
                </Text>
                {status === 'error' ? <Text size="sm" fw={620}>{errorMessage}</Text> : null}
              </>
            ) : status === 'error' ? (
              <>
                <Badge variant="filled" color="red.7" size="sm">Service unavailable</Badge>
                <Text size="sm" fw={620}>{errorMessage ?? 'The local OPAP service is unavailable.'}</Text>
              </>
            ) : status === 'loading' ? (
              <>
                <Badge variant="filled" color="gray.7" size="sm">Loading capabilities</Badge>
                <Text size="sm" fw={620}>
                  OPAP is checking local capabilities. No source inspection or import is available until this finishes.
                </Text>
              </>
            ) : runtime === 'tauri' && capabilities?.source_inspection && capabilities.import_job_preparation ? (
              <>
                <Badge variant="filled" color="opapBlue" size="sm">Desktop preview</Badge>
                <Text size="sm" fw={620}>
                  Source inspection can prepare a blocked job; session import is unavailable. Overview and Daily still show sample values.
                </Text>
              </>
            ) : runtime === 'tauri' ? (
              <>
                <Badge variant="filled" color="gray.7" size="sm">Desktop preview</Badge>
                <Text size="sm" fw={620}>
                  This desktop build reports source inspection or job preparation unavailable. No import can be started.
                </Text>
              </>
            ) : (
              <Text size="sm" fw={620}>The local OPAP service is unavailable. No demo data was substituted.</Text>
            )}
            {status === 'error' && runtime !== 'unavailable' ? (
              <Button size="compact-xs" color="red" variant="light" onClick={retryBootstrap}>
                Retry
              </Button>
            ) : null}
          </Group>
        </section>
        <div className="page-shell">
          <Outlet />
        </div>
      </AppShell.Main>

      <nav className="mobile-bottom-nav" aria-label="Primary navigation">
        {[...primaryNavigation, ...secondaryNavigation].map((item) => {
          const Icon = item.icon;
          const active = location.pathname === item.path;
          return (
            <Link
              key={item.path}
              to={item.path}
              className={active ? 'mobile-nav-link mobile-nav-link-active' : 'mobile-nav-link'}
              aria-current={active ? 'page' : undefined}
            >
              <Icon size={19} strokeWidth={1.9} aria-hidden />
              <span>{item.label.replace(' & About', '')}</span>
            </Link>
          );
        })}
      </nav>
    </AppShell>
  );
}

function getInitials(displayName?: string): string {
  if (!displayName) return 'OP';
  const initials = displayName
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((word) => word[0]?.toUpperCase())
    .join('');
  return initials || 'OP';
}
