import { useTranslation } from 'react-i18next';
import type { SkillLocationRef } from '@/bindings';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import { useEnvironmentStore } from '@/stores/environment';
import { sameEnvironment } from '@/lib/context';
import { environmentRefDisplayName } from '@/lib/environments/presentation';
import { projectDisplayName } from '@/lib/projects/presentation';

export function SkillLocationLabel({ context }: { context: SkillLocationRef }) {
  const { t } = useTranslation();
  const nativeName = useEnvironmentStore((state) => state.environments.find((entry) => (
    sameEnvironment(entry.environment, context.environment)
  ))?.displayName);
  const { projects } = useProjectWorkspace(context.environment);
  const scope = context.scope;
  const project = scope.scope === 'project'
    ? projects.find((item) => item.binding.id === scope.project_id)
    : undefined;
  const scopeLabel = context.scope.scope === 'global'
    ? t('context.global')
    : project ? t('skills.installPath.project', { name: projectDisplayName(project) }) : t('context.projects');
  const environmentLabel = environmentRefDisplayName(context.environment, nativeName, t)
    || t('skills.installPath.nativeEnvironment');

  return <p className="flex min-w-0 flex-wrap gap-x-1 text-xs text-muted-foreground [overflow-wrap:anywhere]">
    <span>{scopeLabel}</span><span aria-hidden="true">·</span><span>{environmentLabel}</span>
  </p>;
}
