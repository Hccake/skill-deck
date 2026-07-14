import type { EnvironmentRef, ProjectInfo } from '@/bindings';
import { sameEnvironment } from '@/lib/context';
import { useProjectStore } from './projects';
import { useWorkspaceContextStore } from './workspace-context';

export interface ProjectRemovalRequest {
  environment: EnvironmentRef;
  projectId: string;
  projectName: string;
  contextRevision: number;
}

function projectName(project: ProjectInfo): string {
  if (project.binding.displayName) return project.binding.displayName;
  const segments = project.binding.nativePath.replace(/\\/g, '/').split('/');
  return segments.at(-1) || project.binding.nativePath;
}

export function captureProjectRemoval(
  environment: EnvironmentRef,
  project: ProjectInfo,
  contextRevision: number,
): ProjectRemovalRequest {
  return {
    environment: { ...environment },
    projectId: project.binding.id,
    projectName: projectName(project),
    contextRevision,
  };
}

export async function confirmProjectRemoval(request: ProjectRemovalRequest): Promise<void> {
  await useProjectStore.getState().remove(request.environment, request.projectId);

  const workspace = useWorkspaceContextStore.getState();
  const stillSelected = workspace.contextRevision === request.contextRevision
    && sameEnvironment(workspace.selectedContext.environment, request.environment)
    && workspace.selectedContext.scope.scope === 'project'
    && workspace.selectedContext.scope.project_id === request.projectId;
  if (stillSelected) {
    workspace.selectGlobal();
  }
}
