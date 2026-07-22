// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import type { OpapClient } from './contracts';
import { createDemoOpapClient } from './demoClient';
import { createTauriOpapClient, type TauriInvoke } from './tauriClient';

interface TauriInternals {
  invoke?: TauriInvoke;
}

export interface OpapRuntimeWindow {
  __TAURI_INTERNALS__?: TauriInternals;
  __TAURI__?: unknown;
  navigator?: Pick<Navigator, 'userAgent'>;
}

/**
 * Selects the local client once at application startup.
 *
 * A real Tauri marker without a callable IPC bridge is a fatal configuration
 * error. It must never fall back to fabricated demo data inside the desktop
 * application. A normal browser receives the explicitly labelled demo client.
 */
export function selectOpapClient(runtime: OpapRuntimeWindow = window): OpapClient {
  const invoke = runtime.__TAURI_INTERNALS__?.invoke;
  if (typeof invoke === 'function') {
    return createTauriOpapClient(invoke.bind(runtime.__TAURI_INTERNALS__));
  }

  if (hasTauriMarker(runtime)) {
    throw new Error('OPAP native runtime was detected, but its IPC bridge is unavailable.');
  }

  return createDemoOpapClient();
}

function hasTauriMarker(runtime: OpapRuntimeWindow): boolean {
  return (
    '__TAURI_INTERNALS__' in runtime ||
    '__TAURI__' in runtime ||
    runtime.navigator?.userAgent.toLowerCase().includes('tauri') === true
  );
}
