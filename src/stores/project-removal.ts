import type { EnvironmentRef, ProjectInfo } from '@/bindings';
import { sameContext } from '@/lib/context';
import { projectWorkspace } from './projects';
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
  const result = await projectWorkspace.execute({
    kind: 'remove',
    environment: request.environment,
    projectId: request.projectId,
    expectedContext: {
      context: {
        environment: request.environment,
        scope: { scope: 'project', project_id: request.projectId },
      },
      revision: request.contextRevision,
    },
  });
  if (result.status === 'failed') throw result.error;
  if (result.status === 'notRun') return false;
  const workspace = useWorkspaceContextStore.getState();
  const expectedContext = {
    environment: request.environment,
    scope: { scope: 'project' as const, project_id: request.projectId },
  };
  if (
    workspace.contextRevision === request.contextRevision
    && sameContext(workspace.selectedContext, expectedContext)
  ) workspace.selectGlobal();
  return true;
}
