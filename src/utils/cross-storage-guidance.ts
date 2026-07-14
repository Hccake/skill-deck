import type { ContextRef } from '@/bindings';
import { environmentKey, useEnvironmentStore } from '@/stores/environment';
import { getProjectStorageOwner } from '@/lib/projectStorage';

export type CrossStorageOperation =
  | 'install'
  | 'update'
  | 'delete'
  | 'manageAgents'
  | 'cleanup'
  | 'copy'
  | 'repair';

type Translate = (key: string, options?: Record<string, unknown>) => string;

export function getCrossStorageFailureGuidance(
  context: ContextRef | null | undefined,
  operation: CrossStorageOperation,
  t: Translate,
): string | null {
  if (!context || context.scope.scope !== 'project') return null;
  const projectId = context.scope.project_id;

  const state = useEnvironmentStore.getState();
  const projects = state.projectsByEnvironment[environmentKey(context.environment)] ?? [];
  const project = projects.find((entry) => entry.id === projectId);
  if (!project) return null;

  const owner = getProjectStorageOwner(context.environment, project.nativePath);
  if (!owner) return null;

  const ownerInfo = state.environments.find(
    (entry) => environmentKey(entry.environment) === environmentKey(owner),
  );
  const environmentLabel = ownerInfo?.displayName
    ?? (owner.kind === 'host' ? t('crossStorage.hostEnvironment') : owner.distro_name);

  return t('crossStorage.failureGuidance', {
    operation: t(`crossStorage.operation.${operation}`),
    environment: environmentLabel,
  });
}

export function appendCrossStorageFailureGuidance(
  message: string,
  context: ContextRef | null | undefined,
  operation: CrossStorageOperation,
  t: Translate,
): string {
  const guidance = getCrossStorageFailureGuidance(context, operation, t);
  if (!guidance || message.includes(guidance)) return message;
  return `${message}\n${guidance}`;
}
