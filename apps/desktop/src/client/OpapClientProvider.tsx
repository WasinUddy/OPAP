// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import {
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react';
import type { AppBootstrap, OpapClient } from './contracts';
import { normalizeApiError } from './errors';
import {
  OpapClientContext,
  type OpapClientContextValue,
} from './opapContext';
import { selectOpapClient } from './runtime';

interface OpapClientProviderProps {
  children: ReactNode;
  /** Test and embedding seam. Production selects its client exactly once. */
  client?: OpapClient;
}

interface ClientSelection {
  client: OpapClient | null;
  errorMessage: string | null;
}

let defaultClientSelection: ClientSelection | undefined;

function selectClient(client?: OpapClient): ClientSelection {
  if (client) return { client, errorMessage: null };
  if (defaultClientSelection) return defaultClientSelection;

  try {
    defaultClientSelection = { client: selectOpapClient(), errorMessage: null };
  } catch {
    defaultClientSelection = {
      client: null,
      errorMessage: 'The OPAP desktop service could not start. No demo data was substituted.',
    };
  }
  return defaultClientSelection;
}

export function OpapClientProvider({ children, client: injectedClient }: OpapClientProviderProps) {
  const [selection] = useState(() => selectClient(injectedClient));
  const [bootstrap, setBootstrap] = useState<AppBootstrap | null>(null);
  const [status, setStatus] = useState<OpapClientContextValue['status']>(selection.client ? 'loading' : 'error');
  const [errorMessage, setErrorMessage] = useState<string | null>(selection.errorMessage);
  const [loadSequence, setLoadSequence] = useState(0);

  const retryBootstrap = useCallback(() => {
    if (!selection.client) return;
    setBootstrap(null);
    setStatus('loading');
    setErrorMessage(null);
    setLoadSequence((sequence) => sequence + 1);
  }, [selection.client]);

  useEffect(() => {
    if (!selection.client) return undefined;

    let active = true;
    void selection.client
      .bootstrap()
      .then((value) => {
        if (!active) return;
        setBootstrap(value);
        setStatus('ready');
      })
      .catch((error: unknown) => {
        if (!active) return;
        setBootstrap(null);
        setStatus('error');
        setErrorMessage(normalizeApiError(error).message);
      });

    return () => {
      active = false;
    };
  }, [loadSequence, selection.client]);

  const value = useMemo<OpapClientContextValue>(
    () => ({
      client: selection.client,
      runtime: selection.client?.runtime ?? 'unavailable',
      status,
      bootstrap,
      capabilities: bootstrap?.capabilities ?? null,
      profiles: bootstrap?.profiles ?? [],
      errorMessage,
      retryBootstrap,
    }),
    [bootstrap, errorMessage, retryBootstrap, selection.client, status],
  );

  return <OpapClientContext.Provider value={value}>{children}</OpapClientContext.Provider>;
}
