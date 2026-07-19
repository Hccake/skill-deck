import {
  collectMutationDiagnostics,
  formatFallbackReason,
  formatMutationError,
  formatMutationWarning,
} from '@/lib/mutation-results';
import type { MutationUnitResult } from '@/bindings';
import type { AppError, EnvironmentInfo, ProjectInfo } from '@/bindings';
import { environmentKey } from '@/lib/context';
import { formatAppError } from '@/utils/format-app-error';

type Translate = (key: string, parameters?: Partial<Record<string, string>>) => string;

export interface MutationResultPresentation {
  summary: string;
  failedUnits: Array<{ unitId: string; skillName: string; message: string }>;
  warnings: string[];
  crossStorageGuidance: boolean;
  diagnostics: string[];
}

export interface MutationPresentationCatalog {
  environments: EnvironmentInfo[];
  projectsByEnvironment: Record<string, ProjectInfo[]>;
}

export interface SkillOperationPresentation {
  unitId: string;
  skillName: string;
  environmentLabel: string;
  scopeLabel: string;
  status: MutationUnitResult['status'];
  retryable: boolean;
  recoveryRequired: boolean;
}

export function presentMutationUnit(
  unit: MutationUnitResult,
  t: Translate,
  catalog?: MutationPresentationCatalog,
): SkillOperationPresentation {
  const key = environmentKey(unit.target.environment);
  const environmentLabel = catalog?.environments
    .find((item) => environmentKey(item.environment) === key)
    ?.displayName
    ?? (unit.target.environment.kind === 'host'
      ? t('mutation.host')
      : unit.target.environment.distro_name);
  let scopeLabel = t('context.global');
  if (unit.target.scope.scope === 'project') {
    const projectId = unit.target.scope.project_id;
    scopeLabel = catalog?.projectsByEnvironment[key]
      ?.find((item) => item.binding.id === projectId)
      ?.binding.displayName
      ?? t('mutation.result.scope.project');
  }

  return {
    unitId: unit.unitId,
    skillName: unit.skillName,
    environmentLabel,
    scopeLabel,
    status: unit.status,
    retryable: unit.retryable,
    recoveryRequired: unit.status === 'recoveryRequired',
  };
}

export function formatWorkflowError(
  error: unknown,
  t: (key: string, parameters?: Record<string, unknown>) => string,
): string {
  if (error && typeof error === 'object' && 'kind' in error) {
    return formatAppError(error as AppError, t as never);
  }
  return t('addSkill.error.unknown');
}

export function presentMutationResults(
  units: MutationUnitResult[],
  t: Translate,
): MutationResultPresentation {
  const failedUnits: MutationResultPresentation['failedUnits'] = [];
  const warnings: string[] = [];
  let crossStorageGuidance = false;

  for (const unit of units) {
    if (unit.status !== 'succeeded') {
      failedUnits.push({
        unitId: unit.unitId,
        skillName: unit.skillName,
        message: unit.error
          ? formatMutationError(unit.error, t)
          : t(`mutation.result.status.${unit.status}`),
      });
    }
    if (unit.fallbackReason) {
      warnings.push(formatFallbackReason(unit.fallbackReason, t));
      if (unit.fallbackReason === 'crossStorageCopyRequired') crossStorageGuidance = true;
    }
    for (const warning of unit.warnings ?? []) {
      warnings.push(formatMutationWarning(warning, t));
    }
  }

  return {
    summary: failedUnits.map(({ skillName, message }) => `${skillName}: ${message}`).join('\n'),
    failedUnits,
    warnings,
    crossStorageGuidance,
    diagnostics: collectMutationDiagnostics(units),
  };
}
