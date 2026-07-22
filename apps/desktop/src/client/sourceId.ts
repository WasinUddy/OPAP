// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

const RANDOM_SOURCE_ID = /^opap-source:[0-9a-f]{32}$/;
const LEGACY_SOURCE_ID = /^opap-source:legacy-[1-9][0-9]{0,18}$/;

/** Matches the opaque identifiers accepted by `opap-service`. */
export function isOpaqueSourceId(value: unknown): value is string {
  return typeof value === 'string' && (RANDOM_SOURCE_ID.test(value) || LEGACY_SOURCE_ID.test(value));
}
