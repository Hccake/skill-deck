import type { SkillLocationRef } from '@/bindings';
import { useEnvironmentStore } from '@/stores/environment';
import { projectSnapshotFor } from '@/stores/projects';
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
  context: SkillLocationRef | null | undefined,
  operation: CrossStorageOperation,
  t: Translate,
): string | null {
  if (!context || context.scope.scope !== 'project') return null;
  const projectId = context.scope.project_id;

  const project = projectSnapshotFor(context.environment).projects.find(
    (entry) => entry.binding.id === projectId,
  );
  if (!project) return null;

  const owner = project.storage.owner;
  if (!owner) return null;

  const ownerInfo = useEnvironmentStore.getState().environments.find(
    (entry) => environmentKey(entry.environment) === environmentKey(owner),
  );
  const environmentLabel = ownerInfo?.displayName
    ?? (owner.kind === 'native' ? t('crossStorage.nativeEnvironment') : owner.distro_name);

  return t('crossStorage.failureGuidance', {
    operation: t(`crossStorage.operation.${operation}`),
    environment: environmentLabel,
  });
}

export function appendCrossStorageFailureGuidance(
  message: string,
  context: SkillLocationRef | null | undefined,
  operation: CrossStorageOperation,
  t: Translate,
): string {
  const guidance = getCrossStorageFailureGuidance(context, operation, t);
  if (!guidance || message.includes(guidance)) return message;
  return `${message}\n${guidance}`;
}
