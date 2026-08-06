import type { EnvironmentRef, ProjectInfo } from '@/bindings';
import { sameEnvironment } from '@/lib/context';
import { useProjectStore } from './projects';
import { useWorkspaceContextStore } from './workspace-context';
import { projectDisplayName } from '@/lib/projects/presentation';

export interface ProjectRemovalRequest {
  environment: EnvironmentRef;
  projectId: string;
  projectName: string;
  contextRevision: number;
}

export function captureProjectRemoval(
  environment: EnvironmentRef,
  project: ProjectInfo,
  contextRevision: number,
): ProjectRemovalRequest {
  return {
    environment: { ...environment },
    projectId: project.binding.id,
    projectName: projectDisplayName(project),
    contextRevision,
  };
}

export async function confirmProjectRemoval(request: ProjectRemovalRequest): Promise<boolean> {
  const result = await useProjectStore.getState().remove(request.environment, request.projectId);
  if (!result) return false;

  const workspace = useWorkspaceContextStore.getState();
  const stillSelected = workspace.contextRevision === request.contextRevision
    && sameEnvironment(workspace.selectedContext.environment, request.environment)
    && workspace.selectedContext.scope.scope === 'project'
    && workspace.selectedContext.scope.project_id === request.projectId;
  if (stillSelected) {
    workspace.selectGlobal();
  }
  return true;
}
