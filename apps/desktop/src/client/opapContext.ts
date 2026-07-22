// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import { createContext, useContext } from 'react';
import type {
  AppBootstrap,
  AppCapabilities,
  ClientRuntime,
  OpapClient,
  ProfileDto,
} from './contracts';

export type BootstrapStatus = 'loading' | 'ready' | 'error';
export type RuntimeStatus = ClientRuntime | 'unavailable';

export interface OpapClientContextValue {
  client: OpapClient | null;
  runtime: RuntimeStatus;
  status: BootstrapStatus;
  bootstrap: AppBootstrap | null;
  capabilities: AppCapabilities | null;
  profiles: ProfileDto[];
  errorMessage: string | null;
  retryBootstrap: () => void;
}

export const OpapClientContext = createContext<OpapClientContextValue | null>(null);

export function useOpapClient(): OpapClientContextValue {
  const context = useContext(OpapClientContext);
  if (!context) throw new Error('useOpapClient must be used inside OpapClientProvider.');
  return context;
}
