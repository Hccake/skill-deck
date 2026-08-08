import type { SkillLocationRef, EnvironmentRef, ProjectInfo } from '@/bindings';
import { sameEnvironment } from '@/lib/context';

export interface CopyTargetFilterInput {
  targetEnvironment: EnvironmentRef;
  sourceContext: SkillLocationRef;
  projects: ProjectInfo[];
  completedProjectIds: ReadonlySet<string>;
}

export function getCopyableProjects({
  targetEnvironment,
  sourceContext,
  projects,
  completedProjectIds,
}: CopyTargetFilterInput): ProjectInfo[] {
  return projects.filter((project) => (
    !completedProjectIds.has(project.binding.id)
    && !(sameEnvironment(targetEnvironment, sourceContext.environment)
      && sourceContext.scope.scope === 'project'
      && project.binding.id === sourceContext.scope.project_id)
  ));
}
