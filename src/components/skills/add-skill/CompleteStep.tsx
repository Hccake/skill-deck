import { useMemo } from 'react';
import { AlertTriangle, CheckCircle2, CircleSlash2, XCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { getCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import type { MutationUnitResult } from '@/bindings';
import type { WizardState } from './types';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';
import { presentMutationUnit } from '@/workflows/mutation-presentation';
import {
  formatFallbackReason,
  formatMutationError,
  formatMutationWarning,
  isRetryableMutationUnit,
} from '@/lib/mutation-results';

interface CompleteStepProps {
  state: WizardState;
  onDone: () => void;
  onRetry?: () => void;
}

const EMPTY_MUTATION_UNITS: MutationUnitResult[] = [];

function statusIcon(unit: MutationUnitResult) {
  switch (unit.status) {
    case 'succeeded':
      return <CheckCircle2 className="h-4 w-4 shrink-0 text-green-600" />;
    case 'failed':
    case 'recoveryRequired':
      return <XCircle className="h-4 w-4 shrink-0 text-destructive" />;
    case 'cancelled':
    case 'skipped':
    case 'notRun':
      return <CircleSlash2 className="h-4 w-4 shrink-0 text-muted-foreground" />;
  }
}

export function CompleteStep({ state, onDone, onRetry }: CompleteStepProps) {
  const { t } = useTranslation();
  const units = state.installResults?.units ?? EMPTY_MUTATION_UNITS;
  const environments = useEnvironmentStore((store) => store.environments);
  const projectsByEnvironment = useProjectStore((store) => store.projectsByEnvironment);
  const presentations = useMemo(
    () => new Map(units.map((unit) => [
      unit.unitId,
      presentMutationUnit(unit, t, { environments, projectsByEnvironment }),
    ])),
    [environments, projectsByEnvironment, t, units],
  );
  const hasFailures = units.some((unit) => unit.status !== 'succeeded');
  const hasRetryable = units.some(isRetryableMutationUnit);
  const failureGuidance = hasFailures
    ? getCrossStorageFailureGuidance(state.context, 'install', t)
    : null;

  return (
    <div className="space-y-4" role="status" aria-live="polite">
      <div className="flex items-center gap-3">
        {hasFailures ? (
          <AlertTriangle className="h-6 w-6 text-warning" />
        ) : (
          <CheckCircle2 className="h-6 w-6 text-green-600" />
        )}
        <h3 className="text-lg font-heading font-bold">
          {hasFailures
            ? t('addSkill.complete.partial')
            : t('addSkill.complete.success', { count: units.length })}
        </h3>
      </div>

      {failureGuidance ? (
        <div className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-sm">
          {failureGuidance}
        </div>
      ) : null}

      <div className="space-y-2">
        {units.map((unit) => {
          const presentation = presentations.get(unit.unitId)!;
          return (
            <div key={unit.unitId} className="rounded-md border border-border/60 bg-card p-3">
              <div className="flex items-start justify-between gap-3">
                <div className="flex min-w-0 items-start gap-2">
                  {statusIcon(unit)}
                  <div className="min-w-0 space-y-1">
                    <p className="truncate text-sm font-medium">{presentation.skillName}</p>
                    <p className="text-xs text-muted-foreground">
                      {t('mutation.result.location', {
                        environment: presentation.environmentLabel,
                        scope: presentation.scopeLabel,
                      })}
                    </p>
                    {unit.fallbackReason ? (
                      <p className="text-xs text-muted-foreground">
                        {formatFallbackReason(unit.fallbackReason, t)}
                      </p>
                    ) : null}
                    {unit.error ? (
                      <p className="text-xs text-destructive" role="alert">
                        {formatMutationError(unit.error, t)}
                      </p>
                    ) : null}
                    {unit.warnings.map((warning, index) => (
                      <p key={`${unit.unitId}:${warning.code}:${index}`} className="text-xs text-warning">
                        {formatMutationWarning(warning, t)}
                      </p>
                    ))}
                  </div>
                </div>
                <Badge variant={unit.status === 'succeeded' ? 'secondary' : 'outline'}>
                  {unit.status === 'recoveryRequired'
                    ? t('addSkill.complete.recoveryRequired')
                    : t(`addSkill.complete.status.${unit.status}`)}
                </Badge>
              </div>
              {unit.recovery ? <RecoveryActions recovery={unit.recovery} /> : null}
            </div>
          );
        })}
      </div>

      <div className="flex justify-end gap-2 pt-2">
        {hasRetryable && onRetry ? (
          <Button variant="outline" onClick={onRetry}>
            {t('addSkill.actions.retry')}
          </Button>
        ) : null}
        <Button onClick={onDone}>{t('addSkill.actions.done')}</Button>
      </div>
    </div>
  );
}
