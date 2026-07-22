// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import { API_ERROR_CODES, type ApiError, type ApiErrorCode } from './contracts';

const apiErrorCodes = new Set<string>(API_ERROR_CODES);
const safeErrorFields = new Set(['display_name', 'profile_id', 'job_id', 'source_id']);

const safeErrorMessages: Record<ApiErrorCode, string> = {
  invalid_request: 'The request is invalid.',
  profile_not_found: 'The requested profile does not exist.',
  job_not_found: 'The requested import job does not exist.',
  conflict: 'The request conflicts with existing local data.',
  source_unavailable: 'The selected source is unavailable; select it again.',
  source_path_invalid: 'The selected source contains an invalid or unsafe path.',
  source_not_supported: 'The selected source is not supported.',
  source_data_invalid: 'The selected source contains invalid device data.',
  source_size_limit_exceeded: 'The selected source exceeds safe inspection limits.',
  capability_unavailable: 'The requested capability is unavailable.',
  job_not_cancellable: 'The import job can no longer be cancelled.',
  storage_unavailable: 'Local storage is unavailable.',
  internal: 'The native OPAP service returned an unexpected error.',
};

export class OpapApiError extends Error {
  readonly code: ApiErrorCode;
  readonly retryable: boolean;
  readonly field?: string;

  constructor(error: ApiError) {
    super(error.message);
    this.name = 'OpapApiError';
    this.code = error.code;
    this.retryable = error.retryable;
    this.field = error.field;
  }

  toDto(): ApiError {
    return {
      code: this.code,
      message: this.message,
      retryable: this.retryable,
      ...(this.field === undefined ? {} : { field: this.field }),
    };
  }
}

export function isApiError(value: unknown): value is ApiError {
  if (!isRecord(value)) return false;
  return (
    typeof value.code === 'string' &&
    apiErrorCodes.has(value.code) &&
    typeof value.message === 'string' &&
    typeof value.retryable === 'boolean' &&
    (value.field === undefined || typeof value.field === 'string')
  );
}

/**
 * Converts native command rejections into one stable renderer error. Unknown
 * values are deliberately replaced with a generic message: arbitrary native
 * strings can contain filesystem paths and must not become display text.
 */
export function normalizeApiError(value: unknown): OpapApiError {
  const candidate: unknown = value instanceof OpapApiError ? value.toDto() : value;
  if (isApiError(candidate)) {
    return new OpapApiError({
      code: candidate.code,
      message: safeErrorMessages[candidate.code],
      retryable: candidate.retryable,
      ...(candidate.field !== undefined && safeErrorFields.has(candidate.field)
        ? { field: candidate.field }
        : {}),
    });
  }

  return new OpapApiError({
    code: 'internal',
    message: safeErrorMessages.internal,
    retryable: false,
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
