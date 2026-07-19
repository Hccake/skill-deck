import { describe, expect, it } from 'vitest';
import type { ErrorReport, MutationWarning } from '@/bindings';
import {
  formatFallbackReason,
  formatMutationError,
  formatMutationWarning,
  isRetryableMutationUnit,
} from '../mutation-results';

const t = (key: string, parameters?: Partial<Record<string, string>>) =>
  `${key}${parameters && Object.keys(parameters).length > 0 ? JSON.stringify(parameters) : ''}`;

function errorReport(overrides: Partial<ErrorReport> = {}): ErrorReport {
  return {
    code: 'executionFailed',
    parameters: { operation: 'install' },
    field: null,
    severity: 'error',
    retryable: true,
    technicalDetails: 'permission denied at /secret/path',
    environment: null,
    context: null,
    unitId: 'demo',
    recoveryResourceId: null,
    displayPaths: [],
    ...overrides,
  };
}

describe('mutation result presentation', () => {
  it('formats public errors from stable codes and parameters without exposing diagnostics', () => {
    expect(formatMutationError(errorReport(), t)).toBe(
      'mutation.result.errors.executionFailed{"operation":"install"}',
    );
    expect(formatMutationError(errorReport(), t)).not.toContain('permission denied');
  });

  it('falls back safely for an unknown runtime error code', () => {
    expect(formatMutationError(errorReport({ code: 'futureCode' as never }), t)).toBe(
      'mutation.result.errors.unknown',
    );
  });

  it('formats fallback reasons and warnings from stable codes', () => {
    expect(formatFallbackReason('crossStorageCopyRequired', t)).toBe(
      'mutation.result.fallbacks.crossStorageCopyRequired',
    );
    expect(formatMutationWarning({
      code: 'backupCleanupFailed',
      parameters: { path: '/backup' },
      technicalDetails: 'raw cleanup failure',
    } satisfies MutationWarning, t)).toBe(
      'mutation.result.warnings.backupCleanupFailed{"path":"/backup"}',
    );
  });

  it('allows ordinary retry only for explicitly retryable failed, cancelled, or not-run units', () => {
    expect(isRetryableMutationUnit({ status: 'failed', retryable: true })).toBe(true);
    expect(isRetryableMutationUnit({ status: 'cancelled', retryable: true })).toBe(true);
    expect(isRetryableMutationUnit({ status: 'notRun', retryable: true })).toBe(true);
    expect(isRetryableMutationUnit({ status: 'recoveryRequired', retryable: true })).toBe(false);
    expect(isRetryableMutationUnit({ status: 'failed', retryable: false })).toBe(false);
  });
});
