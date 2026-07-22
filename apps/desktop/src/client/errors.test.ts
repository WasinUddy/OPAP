// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

import { API_ERROR_CODES } from './contracts';
import { isApiError, normalizeApiError, OpapApiError } from './errors';

describe('ApiError contract', () => {
  it.each(API_ERROR_CODES)('accepts the Rust wire error code %s', (code) => {
    const dto = { code, message: 'Safe message', retryable: false };

    expect(isApiError(dto)).toBe(true);
    expect(normalizeApiError(dto).toDto()).toMatchObject({ code, retryable: false });
    expect(normalizeApiError(dto).message).not.toBe(dto.message);
  });

  it('normalizes an optional allowlisted field and existing typed error', () => {
    const error = new OpapApiError({
      code: 'invalid_request',
      message: 'Required',
      retryable: false,
      field: 'display_name',
    });

    expect(normalizeApiError(error)).not.toBe(error);
    expect(normalizeApiError(error).toDto()).toEqual({
      code: 'invalid_request',
      message: 'The request is invalid.',
      retryable: false,
      field: 'display_name',
    });
  });

  it('replaces path-bearing content in an existing typed error', () => {
    const normalized = normalizeApiError(
      new OpapApiError({
        code: 'internal',
        message: '/Users/alice/private/card',
        retryable: false,
        field: '/Users/alice/private/card',
      }),
    );

    expect(normalized.toDto()).toEqual({
      code: 'internal',
      message: 'The native OPAP service returned an unexpected error.',
      retryable: false,
    });
  });

  it('replaces path-bearing content in a well-formed native envelope', () => {
    const normalized = normalizeApiError({
      code: 'source_unavailable',
      message: 'Could not open /Users/alice/private/card',
      retryable: true,
      field: '/Users/alice/private/card',
    });

    expect(normalized.toDto()).toEqual({
      code: 'source_unavailable',
      message: 'The selected source is unavailable; select it again.',
      retryable: true,
    });
    expect(JSON.stringify(normalized.toDto())).not.toContain('/Users');
  });

  it.each([
    null,
    'native failure',
    new Error('native failure'),
    { code: 'unknown', message: 'failure', retryable: false },
    { code: 'internal', message: 42, retryable: false },
    { code: 'internal', message: 'failure', retryable: 'no' },
    { code: 'internal', message: 'failure', retryable: false, field: 42 },
  ])('normalizes malformed rejection %# to a safe internal error', (value) => {
    expect(normalizeApiError(value).toDto()).toEqual({
      code: 'internal',
      message: 'The native OPAP service returned an unexpected error.',
      retryable: false,
    });
  });
});
