import type {
  ErrorReport,
  FallbackReasonCode,
  MutationUnitResult,
  MutationWarning,
  OperationErrorCode,
} from '@/bindings';

type Translate = (
  key: string,
  parameters?: Partial<Record<string, string>>,
) => string;

const ERROR_CODES = new Set<OperationErrorCode>([
  'validation',
  'environmentUnavailable',
  'environmentChanged',
  'contextChanged',
  'storageUnsupported',
  'capabilityUnavailable',
  'unsafePath',
  'unsafeSourceLink',
  'selfCopy',
  'payloadSessionExpired',
  'staleContext',
  'staleRegistry',
  'staleEnvironment',
  'stalePayload',
  'staleTarget',
  'externalLockChanged',
  'mutationCancelled',
  'executionFailed',
  'restoreFailed',
  'recoveryRequired',
  'configurationReadOnly',
  'configurationCorrupted',
]);

export function formatMutationError(error: ErrorReport, t: Translate): string {
  const code = ERROR_CODES.has(error.code) ? error.code : 'unknown';
  const parameters = code !== 'unknown' && Object.keys(error.parameters).length > 0
    ? error.parameters
    : undefined;
  return t(`mutation.result.errors.${code}`, parameters);
}

export function formatFallbackReason(reason: FallbackReasonCode, t: Translate): string {
  return t(`mutation.result.fallbacks.${reason}`);
}

export function formatMutationWarning(warning: MutationWarning, t: Translate): string {
  const parameters = Object.keys(warning.parameters).length > 0
    ? warning.parameters
    : undefined;
  return t(`mutation.result.warnings.${warning.code}`, parameters);
}

export function isRetryableMutationUnit(
  unit: Pick<MutationUnitResult, 'status' | 'retryable'>,
): boolean {
  return unit.retryable
    && (unit.status === 'failed' || unit.status === 'cancelled' || unit.status === 'notRun');
}

export function collectMutationDiagnostics(units: MutationUnitResult[]): string[] {
  const diagnostics: string[] = [];
  for (const unit of units) {
    if (unit.error?.technicalDetails) {
      diagnostics.push(`${unit.unitId}: ${unit.error.technicalDetails}`);
    }
    for (const warning of unit.warnings ?? []) {
      if (warning.technicalDetails) {
        diagnostics.push(`${unit.unitId}: ${warning.technicalDetails}`);
      }
    }
  }
  return diagnostics;
}
