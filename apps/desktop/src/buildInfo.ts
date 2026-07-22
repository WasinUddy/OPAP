const repositoryUrl = 'https://github.com/WasinUddy/OPAP';
const configuredRevision = import.meta.env.VITE_OPAP_SOURCE_REVISION?.trim();
const validRevision = configuredRevision && /^[0-9a-f]{7,40}$/i.test(configuredRevision)
  ? configuredRevision
  : undefined;

export const buildInfo = {
  version: '0.1.0-preview',
  sourceHref: validRevision ? `${repositoryUrl}/commit/${validRevision}` : repositoryUrl,
  sourceLabel: validRevision
    ? `Source revision ${validRevision.slice(0, 12)}`
    : 'Preview source repository · revision unavailable',
} as const;
