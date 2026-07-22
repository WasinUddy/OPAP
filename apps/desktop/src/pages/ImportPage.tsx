import {
  Alert,
  Badge,
  Box,
  Button,
  Divider,
  Group,
  Loader,
  Paper,
  Select,
  Stack,
  Stepper,
  Table,
  Text,
  ThemeIcon,
  Title,
} from '@mantine/core';
import {
  AlertCircle,
  AlertTriangle,
  Ban,
  Check,
  CircleCheck,
  FolderOpen,
  HardDrive,
  Info,
  LockKeyhole,
  RefreshCw,
  Usb,
} from 'lucide-react';
import { type RefObject, useEffect, useRef, useState } from 'react';
import type { ImportJobDto, ProfileDto, SourceInspection } from '../client';
import { normalizeApiError, OpapApiError, useOpapClient } from '../client';
import sleepingBreath from '../assets/sleeping-breath.svg';

type Operation = 'select' | 'prepare' | 'list' | 'cancel';
type RetryAction = Operation;

interface WorkflowError {
  message: string;
  retry: RetryAction;
}

export function ImportPage() {
  const {
    capabilities,
    client,
    errorMessage: bootstrapError,
    profiles,
    retryBootstrap,
    runtime,
    status: bootstrapStatus,
  } = useOpapClient();
  const [active, setActive] = useState(0);
  const [inspection, setInspection] = useState<SourceInspection | null>(null);
  const [currentJob, setCurrentJob] = useState<ImportJobDto | null>(null);
  const [jobs, setJobs] = useState<ImportJobDto[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<number | null>(null);
  const [operation, setOperation] = useState<Operation | null>(null);
  const [workflowError, setWorkflowError] = useState<WorkflowError | null>(null);
  const [pickerCancelled, setPickerCancelled] = useState(false);
  const stageHeadingRef = useRef<HTMLHeadingElement>(null);
  const previousFocusStage = useRef(`${active}:${currentJob?.status ?? ''}`);

  const selectedProfile = resolveProfile(profiles, selectedProfileId);
  const demo = runtime === 'demo';

  useEffect(() => {
    const nextStage = `${active}:${currentJob?.status ?? ''}`;
    if (previousFocusStage.current === nextStage) return;
    previousFocusStage.current = nextStage;
    stageHeadingRef.current?.focus();
  }, [active, currentJob?.status]);

  async function selectSource() {
    if (!client || !capabilities?.source_inspection) {
      setWorkflowError({
        message: 'Source inspection is unavailable in this OPAP build.',
        retry: 'select',
      });
      return;
    }

    setOperation('select');
    setWorkflowError(null);
    setPickerCancelled(false);

    try {
      const result = await client.selectNativeSource();
      if (result === null) {
        setInspection(null);
        setPickerCancelled(true);
        setActive(0);
        return;
      }

      setInspection(result);
      setCurrentJob(null);
      setJobs([]);
      setActive(1);
    } catch (error: unknown) {
      setWorkflowError({ message: safeErrorMessage(error, 'OPAP could not inspect the selected source.'), retry: 'select' });
    } finally {
      setOperation(null);
    }
  }

  async function prepareJob() {
    if (!client || !inspection || !selectedProfile) return;

    setOperation('prepare');
    setWorkflowError(null);

    try {
      const response = await client.prepareImportJob({
        profile_id: selectedProfile.id,
        source_id: inspection.source_id,
      });
      setCurrentJob(response.job);
      setActive(2);

      try {
        setJobs(await client.listImportJobs(selectedProfile.id));
      } catch (error: unknown) {
        setWorkflowError({
          message: safeErrorMessage(error, 'The blocked job was prepared, but its history could not be refreshed.'),
          retry: 'list',
        });
      }
    } catch (error: unknown) {
      setWorkflowError({ message: safeErrorMessage(error, 'OPAP could not prepare the import job.'), retry: 'prepare' });
    } finally {
      setOperation(null);
    }
  }

  async function listJobs() {
    if (!client || !selectedProfile) return;

    setOperation('list');
    setWorkflowError(null);
    try {
      setJobs(await client.listImportJobs(selectedProfile.id));
    } catch (error: unknown) {
      setWorkflowError({ message: safeErrorMessage(error, 'OPAP could not refresh import job history.'), retry: 'list' });
    } finally {
      setOperation(null);
    }
  }

  async function cancelJob() {
    if (!client || !selectedProfile || !currentJob) return;

    setOperation('cancel');
    setWorkflowError(null);
    try {
      const cancelled = await client.cancelImportJob(selectedProfile.id, currentJob.id);
      setCurrentJob(cancelled);
      setJobs((existing) => replaceJob(existing, cancelled));
    } catch (error: unknown) {
      setWorkflowError({ message: safeErrorMessage(error, 'OPAP could not cancel this job.'), retry: 'cancel' });
    } finally {
      setOperation(null);
    }
  }

  function retryWorkflow() {
    if (!workflowError) return;
    if (workflowError.retry === 'select') void selectSource();
    if (workflowError.retry === 'prepare') void prepareJob();
    if (workflowError.retry === 'list') void listJobs();
    if (workflowError.retry === 'cancel') void cancelJob();
  }

  if (bootstrapStatus === 'loading') {
    return (
      <Paper withBorder p="xl" maw={1000} mx="auto" role="status" aria-label="Loading OPAP import capabilities">
        <Group justify="center" py={48}><Loader size="sm" /><Text c="dimmed">Loading local import capabilities…</Text></Group>
      </Paper>
    );
  }

  if (bootstrapStatus === 'error' || !client || !capabilities) {
    return (
      <Alert maw={1000} mx="auto" color="red" icon={<AlertCircle size={19} />} title="Import service unavailable">
        <Text size="sm">{bootstrapError ?? 'OPAP could not load the local import service.'}</Text>
        {client ? <Button mt="md" size="xs" color="red" variant="light" onClick={retryBootstrap}>Try again</Button> : null}
      </Alert>
    );
  }

  const recognized = inspection?.recognized === true && inspection.importer_id !== undefined;
  const canPrepare = recognized
    && capabilities.import_job_preparation
    && inspection.session_import.available === false
    && selectedProfile !== null;

  return (
    <Stack gap="lg" maw={1000} mx="auto" w="100%">
      <Box>
        <Title order={1} className="mobile-page-title">Prepare an import</Title>
        <Text size="sm" c="dimmed">
          Inspect a source, review only privacy-safe details, then record a blocked job. Session data import is not available yet.
        </Text>
      </Box>

      {demo ? (
        <Alert color="yellow" variant="light" icon={<Info size={19} />} title="Fabricated browser demonstration">
          The source, device details, and job states on this screen are built-in samples. No folder picker, CPAP card, or local storage is used.
        </Alert>
      ) : null}

      {workflowError ? (
        <Alert color="red" icon={<AlertCircle size={19} />} title="This action did not finish">
          <Text size="sm">{workflowError.message}</Text>
          <Button mt="md" size="xs" color="red" variant="light" leftSection={<RefreshCw size={14} />} onClick={retryWorkflow}>
            Try again
          </Button>
        </Alert>
      ) : null}

      <Paper withBorder p={{ base: 'md', sm: 'xl' }}>
        <Stepper active={active} color="opapBlue" iconSize={34} mb={36} allowNextStepsSelect={false}>
          <Stepper.Step label="Select source" description={demo ? 'Fabricated sample' : 'Native folder picker'} icon={<FolderOpen size={17} />} completedIcon={<Check size={17} />} />
          <Stepper.Step label="Review" description="Privacy-safe details" icon={<HardDrive size={17} />} completedIcon={<Check size={17} />} />
          <Stepper.Step label="Job status" description="No session import" icon={<Ban size={17} />} completedIcon={<Check size={17} />} />
        </Stepper>

        {active === 0 ? (
          <Stack gap="lg">
            <Title order={2} fz="lg" ref={stageHeadingRef} tabIndex={-1}>Select a source</Title>
            <Select
              label="Therapy profile"
              description="The prepared job is associated with this local profile."
              data={profiles.map((profile) => ({ value: String(profile.id), label: profile.display_name }))}
              value={selectedProfile ? String(selectedProfile.id) : null}
              onChange={(value) => setSelectedProfileId(value ? Number(value) : null)}
              disabled={profiles.length <= 1 || operation !== null}
              allowDeselect={false}
              maw={420}
            />

            {pickerCancelled ? (
              <Alert role="status" color="blue" variant="light" icon={<Info size={19} />} title="No folder selected">
                Nothing was inspected or saved. Choose a source whenever you are ready.
              </Alert>
            ) : null}

            {profiles.length === 0 ? (
              <Alert color="yellow" icon={<AlertTriangle size={19} />} title="A profile is required">
                Create a local profile before preparing an import job.
              </Alert>
            ) : !capabilities.source_inspection ? (
              <Alert color="yellow" icon={<AlertTriangle size={19} />} title="Source inspection unavailable">
                This OPAP build does not expose source inspection. No folder can be selected or read.
              </Alert>
            ) : (
              <button className="drop-zone" onClick={() => void selectSource()} type="button" disabled={operation !== null}>
                <ThemeIcon variant="light" color="opapBlue" size={54} radius="xl"><Usb size={25} /></ThemeIcon>
                <div>
                  <Badge variant={demo ? 'outline' : 'light'} color={demo ? 'yellow.9' : 'opapBlue'} mb="sm">
                    {demo ? 'Fabricated browser source' : 'Local desktop inspection'}
                  </Badge>
                  <Text fw={680} fz="lg">{demo ? 'Inspect the fabricated demo source' : 'Choose a CPAP card folder'}</Text>
                  <Text size="sm" c="dimmed" mt={5}>
                    {demo
                      ? 'Returns built-in sample metadata without opening a folder picker.'
                      : 'The native picker keeps the folder path outside the renderer.'}
                  </Text>
                </div>
                <span className="drop-zone-button">
                  {operation === 'select' ? 'Inspecting…' : demo ? 'Inspect fabricated source' : 'Choose folder securely'}
                </span>
                <Text size="xs" c="dimmed">No folder path or full device serial is shown in this interface</Text>
              </button>
            )}

            <Group justify="center" gap={6}>
              <LockKeyhole size={14} color="#66717f" />
              <Text size="xs" c="dimmed">
                {demo ? 'Fabricated in memory · no file access or persistence' : 'Folder access stays inside the local Rust service'}
              </Text>
            </Group>
          </Stack>
        ) : null}

        {active === 1 && inspection ? (
          <SourceReview
            inspection={inspection}
            demo={demo}
            selectedProfile={selectedProfile}
            canPrepare={canPrepare}
            preparationAvailable={capabilities.import_job_preparation}
            preparing={operation === 'prepare'}
            busy={operation !== null}
            headingRef={stageHeadingRef}
            onBack={() => setActive(0)}
            onPrepare={() => void prepareJob()}
          />
        ) : null}

        {active === 2 && currentJob ? (
          <JobStatus
            currentJob={currentJob}
            jobs={jobs}
            demo={demo}
            cancelling={operation === 'cancel'}
            refreshing={operation === 'list'}
            busy={operation !== null}
            headingRef={stageHeadingRef}
            onCancel={() => void cancelJob()}
            onRefresh={() => void listJobs()}
            onAnother={() => {
              setActive(0);
              setInspection(null);
              setCurrentJob(null);
              setJobs([]);
              setWorkflowError(null);
            }}
          />
        ) : null}
      </Paper>
    </Stack>
  );
}

interface SourceReviewProps {
  inspection: SourceInspection;
  demo: boolean;
  selectedProfile: ProfileDto | null;
  canPrepare: boolean;
  preparationAvailable: boolean;
  preparing: boolean;
  busy: boolean;
  headingRef: RefObject<HTMLHeadingElement | null>;
  onBack: () => void;
  onPrepare: () => void;
}

function SourceReview({ inspection, demo, selectedProfile, canPrepare, preparationAvailable, preparing, busy, headingRef, onBack, onPrepare }: SourceReviewProps) {
  const recognized = inspection.recognized && inspection.importer_id !== undefined;

  return (
    <Stack gap="lg">
      <Title order={2} fz="lg" ref={headingRef} tabIndex={-1}>Review selected source</Title>
      <Alert
        color={recognized ? 'opapTeal' : 'yellow'}
        variant="light"
        icon={recognized ? <CircleCheck size={19} /> : <AlertTriangle size={19} />}
        title={recognized ? 'Supported source recognized' : 'This source is not supported yet'}
      >
        {recognized
          ? 'Only redacted source and device metadata is shown below. No therapy session has been imported.'
          : 'Nothing was written. Choose another source or keep this page open while support is expanded.'}
      </Alert>

      <Paper withBorder p="lg" className="detected-device">
        <Group justify="space-between" align="flex-start" wrap="wrap" gap="lg">
          <Group align="flex-start" gap="md">
            <ThemeIcon variant="light" size={44} radius="md"><HardDrive size={21} /></ThemeIcon>
            <div>
              <Text fw={680}>{inspection.device?.model || (recognized ? 'Recognized CPAP source' : 'Unrecognized source')}</Text>
              <Text size="sm" c="dimmed" mt={2}>
                {inspection.device?.brand || 'Device maker unavailable'} · full serial hidden
              </Text>
            </div>
          </Group>
          <Badge variant={demo ? 'outline' : 'light'} color={demo ? 'yellow.9' : recognized ? 'opapTeal' : 'yellow'}>
            {demo ? 'Fabricated sample' : recognized ? 'Locally inspected' : 'Unsupported'}
          </Badge>
        </Group>
        <Divider my="lg" />
        <div className="device-detail-grid">
          <div><Text size="xs" c="dimmed">Privacy-safe source label</Text><Text size="sm" fw={620} mt={3}>{inspection.source_label}</Text></div>
          <div><Text size="xs" c="dimmed">Inventory</Text><Text size="sm" fw={620} mt={3}>{inspection.files.toLocaleString()} files · {inspection.directories.toLocaleString()} folders</Text></div>
          <div><Text size="xs" c="dimmed">Inspected size</Text><Text size="sm" fw={620} mt={3}>{formatBytes(inspection.total_bytes)}</Text></div>
        </div>
      </Paper>

      {inspection.warnings.map((warning) => (
        <Alert
          key={`${warning.code}-${warning.message}`}
          color={warning.severity === 'warning' ? 'yellow' : 'blue'}
          variant="light"
          icon={warning.severity === 'warning' ? <AlertTriangle size={18} /> : <Info size={18} />}
          title={warning.severity === 'warning' ? 'Inspection warning' : 'Inspection note'}
        >
          {warning.message}
        </Alert>
      ))}

      {recognized ? (
        <Alert
          color="yellow"
          variant="light"
          icon={<Ban size={19} />}
          title={preparationAvailable ? 'Session import is not available' : 'Job preparation is not available'}
        >
          {preparationAvailable
            ? `Preparing the next step creates a blocked administrative job for ${selectedProfile?.display_name ?? 'the selected profile'}. It does not parse, copy, or save therapy sessions.`
            : 'This OPAP build can review the source, but it cannot create an import job or save therapy sessions.'}
        </Alert>
      ) : null}

      <Group justify="space-between">
        <Button variant="subtle" color="gray" onClick={onBack} disabled={busy}>Choose another source</Button>
        {recognized ? (
          <Button onClick={onPrepare} loading={preparing} disabled={!canPrepare || busy} leftSection={<LockKeyhole size={16} />}>
            Prepare blocked import job
          </Button>
        ) : null}
      </Group>
    </Stack>
  );
}

interface JobStatusProps {
  currentJob: ImportJobDto;
  jobs: ImportJobDto[];
  demo: boolean;
  cancelling: boolean;
  refreshing: boolean;
  busy: boolean;
  headingRef: RefObject<HTMLHeadingElement | null>;
  onCancel: () => void;
  onRefresh: () => void;
  onAnother: () => void;
}

function JobStatus({ currentJob, jobs, demo, cancelling, refreshing, busy, headingRef, onCancel, onRefresh, onAnother }: JobStatusProps) {
  const presentation = jobPresentation(currentJob.status);
  return (
    <Stack gap="lg">
      <Stack align="center" gap="md" py={{ base: 'md', sm: 'lg' }} aria-live="polite">
        <Box component="img" src={sleepingBreath} alt="" aria-hidden className="import-illustration" />
        <ThemeIcon color={presentation.color} variant="light" size={54} radius="xl">
          {jobStatusIcon(currentJob.status)}
        </ThemeIcon>
        <div>
          <Title order={2} fz={23} ta="center" ref={headingRef} tabIndex={-1}>{presentation.title}</Title>
          <Text c="dimmed" ta="center" mt={7} maw={610}>
            {presentation.description}
          </Text>
        </div>
        <Badge color={statusColor(currentJob.status)} variant={currentJob.status === 'blocked' ? 'outline' : 'light'} size="lg">
          {demo ? 'Fabricated job · ' : ''}{formatStatus(currentJob.status)}
        </Badge>
        {currentJob.unavailable_reason ? (
          <Text size="xs" c="dimmed" ta="center">Capability status: session importer unavailable</Text>
        ) : null}
        {currentJob.can_cancel ? (
          <Button color="red" variant="light" loading={cancelling} disabled={busy} onClick={onCancel}>
            {currentJob.status === 'blocked' ? 'Cancel blocked job' : 'Cancel job'}
          </Button>
        ) : null}
      </Stack>

      <Divider />

      <Group justify="space-between" align="flex-end" wrap="wrap">
        <div>
          <Text fw={680}>Import job history</Text>
          <Text size="sm" c="dimmed">Status records only. This list does not represent imported therapy sessions.</Text>
        </div>
        <Button size="xs" variant="default" loading={refreshing} disabled={busy} leftSection={<RefreshCw size={14} />} onClick={onRefresh}>
          Refresh jobs
        </Button>
      </Group>

      {jobs.length ? (
        <Table.ScrollContainer minWidth={560}>
          <Table withTableBorder striped verticalSpacing="sm">
            <Table.Thead><Table.Tr><Table.Th>Job</Table.Th><Table.Th>Safe source label</Table.Th><Table.Th>Attempt</Table.Th><Table.Th>Status</Table.Th></Table.Tr></Table.Thead>
            <Table.Tbody>
              {jobs.map((job) => (
                <Table.Tr key={job.id}>
                  <Table.Td fw={620}>#{job.id}</Table.Td>
                  <Table.Td>{job.source_label}</Table.Td>
                  <Table.Td className="numeric-cell">{job.attempt}</Table.Td>
                  <Table.Td><Badge color={statusColor(job.status)} variant={job.status === 'blocked' ? 'outline' : 'light'}>{formatStatus(job.status)}</Badge></Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </Table.ScrollContainer>
      ) : (
        <Alert color="blue" variant="light" icon={<Info size={18} />}>No job history was returned for this profile.</Alert>
      )}

      <Group justify="flex-start">
        <Button variant="subtle" color="gray" onClick={onAnother} disabled={busy}>Inspect another source</Button>
      </Group>
    </Stack>
  );
}

function resolveProfile(profiles: ProfileDto[], selectedProfileId: number | null): ProfileDto | null {
  return profiles.find((profile) => profile.id === selectedProfileId) ?? profiles[0] ?? null;
}

function safeErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof OpapApiError) return normalizeApiError(error).message;
  return fallback;
}

function replaceJob(jobs: ImportJobDto[], replacement: ImportJobDto): ImportJobDto[] {
  const found = jobs.some((job) => job.id === replacement.id);
  return found ? jobs.map((job) => (job.id === replacement.id ? replacement : job)) : [replacement, ...jobs];
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes.toLocaleString()} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

function formatStatus(status: ImportJobDto['status']): string {
  return status.charAt(0).toUpperCase() + status.slice(1);
}

function statusColor(status: ImportJobDto['status']): string {
  if (status === 'blocked') return 'orange.9';
  if (status === 'cancelled') return 'gray';
  if (status === 'failed') return 'red';
  if (status === 'completed') return 'opapTeal';
  return 'opapBlue';
}

function jobPresentation(status: ImportJobDto['status']): { title: string; description: string; color: string } {
  if (status === 'blocked') {
    return {
      title: 'Import job prepared and blocked',
      description: 'No therapy sessions were imported. The job cannot run until a tested session importer is available in a future release.',
      color: 'yellow',
    };
  }
  if (status === 'cancelled') {
    return {
      title: 'Import job cancelled',
      description: 'No therapy sessions were imported by this preparation screen. The administrative job is closed and cannot run.',
      color: 'gray',
    };
  }
  if (status === 'running') {
    return {
      title: 'Import job reports running',
      description: 'This preparation screen did not start the job and makes no claim that therapy sessions were imported.',
      color: 'opapBlue',
    };
  }
  if (status === 'completed') {
    return {
      title: 'Import job reports completed',
      description: 'This preparation screen did not execute the job. It does not verify or display imported therapy sessions.',
      color: 'opapTeal',
    };
  }
  return {
    title: 'Import job failed',
    description: 'The job reports a failure. This screen does not claim that any therapy session was imported.',
    color: 'red',
  };
}

function jobStatusIcon(status: ImportJobDto['status']) {
  if (status === 'blocked') return <Ban size={25} />;
  if (status === 'cancelled' || status === 'completed') return <Check size={25} />;
  if (status === 'failed') return <AlertCircle size={25} />;
  return <RefreshCw size={25} />;
}
