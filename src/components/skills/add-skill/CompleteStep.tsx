import { useMemo } from 'react';
import { AlertTriangle, CheckCircle2, ChevronRight, CircleSlash2, XCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from '@/components/ui/collapsible';
import { getCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import type { MutationUnitResult } from '@/bindings';
import type { WizardState } from './types';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';
import { presentMutationUnit } from '@/workflows/mutation-presentation';
import {
  collectMutationDiagnostics,
  formatFallbackReason,
  formatMutationError,
  formatMutationWarning,
} from '@/lib/mutation-results';

interface CompleteStepProps {
  state: WizardState;
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

function MutationDiagnosticDetails({
  unit,
  label,
}: {
  unit: MutationUnitResult;
  label: string;
}) {
  if (unit.status === 'succeeded') return null;
  const diagnostics = collectMutationDiagnostics([unit]);
  if (diagnostics.length === 0) return null;

  return (
    <Collapsible className="pt-0.5">
      <CollapsibleTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="group -ml-2 h-6 px-2 text-xs font-normal text-muted-foreground"
        >
          <ChevronRight className="transition-transform group-data-[state=open]:rotate-90" />
          {label}
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="mt-1 space-y-1 bg-muted/25 px-2 py-1.5 font-mono text-[11px] leading-4 text-muted-foreground">
          {diagnostics.map((diagnostic, index) => (
            <p key={`${unit.unitId}:diagnostic:${index}`} className="break-all whitespace-pre-wrap">
              {diagnostic}
            </p>
          ))}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

export function CompleteStep({ state }: CompleteStepProps) {
  const { t } = useTranslation();
  const units = state.installResults?.units ?? EMPTY_MUTATION_UNITS;
  const warnings = state.installResults?.warnings ?? [];
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

      {warnings.map((warning) => (
        <p key={warning} className="text-sm text-warning" role="status">
          {t(`addSkill.complete.warnings.${warning}`)}
        </p>
      ))}

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
                    <MutationDiagnosticDetails
                      unit={unit}
                      label={t('addSkill.complete.errorDetails')}
                    />
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
    </div>
  );
}
