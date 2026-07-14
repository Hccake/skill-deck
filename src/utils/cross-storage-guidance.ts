import type { ContextRef } from '@/bindings';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';
import { environmentKey } from '@/lib/context';

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

  const projects = useProjectStore.getState().projectsByEnvironment[
    environmentKey(context.environment)
  ] ?? [];
  const project = projects.find((entry) => entry.binding.id === projectId);
  if (!project) return null;

  const owner = project.storage.owner;
  if (!owner) return null;

  const ownerInfo = useEnvironmentStore.getState().environments.find(
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
