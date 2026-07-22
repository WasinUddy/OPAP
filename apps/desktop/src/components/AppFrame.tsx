import {
  AppShell,
  Avatar,
  Badge,
  Box,
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
  '/import': { eyebrow: 'Simulated workflow', title: 'Preview an import' },
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
            <Badge variant="filled" color="yellow.7" visibleFrom="xs" leftSection={<FlaskConical size={13} />}>
              Demo data
            </Badge>
            <Menu position="bottom-end" shadow="md" width={230}>
              <Menu.Target>
                <UnstyledButton className="profile-button" aria-label="Open profile menu">
                  <Group gap={8} wrap="nowrap">
                    <Avatar size={34} radius="xl" color="opapBlue">DP</Avatar>
                    <Box visibleFrom="xs">
                      <Text size="sm" fw={650} lh={1.15}>Demo profile</Text>
                      <Text size="xs" c="dimmed" lh={1.15}>Fabricated preview</Text>
                    </Box>
                    <ChevronDown size={14} aria-hidden />
                  </Group>
                </UnstyledButton>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Label>Preview profile</Menu.Label>
                <Menu.Item disabled leftSection={<Avatar size={22} color="opapBlue">DP</Avatar>}>
                  Demo profile · sample data
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
        <section className="demo-banner" role="note" aria-label="Demo data notice">
          <Group justify="center" gap={9} wrap="nowrap">
            <Badge variant="filled" color="yellow.8" size="sm">Demo / sample data</Badge>
            <Text size="sm" fw={620}>
              Every clinical value and import result shown is fabricated. Real CPAP card reading is not connected in this preview.
            </Text>
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
