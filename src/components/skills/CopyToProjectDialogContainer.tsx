import { useMemo } from 'react';
import { listSkills } from '@/hooks/useTauriApi';
import { contextKey } from '@/lib/context';
import { useEnvironmentStore } from '@/stores/environment';
import { useCopyAgentSelection } from '@/hooks/useCopyAgentSelection';
import { projectWorkspace } from '@/stores/projects';
import { useProjectCatalog } from '@/hooks/useProjectWorkspace';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { executeSkillCopy } from '@/workflows/skill-copy';
import { CopyToProjectDialog } from './CopyToProjectDialog';
import type { SkillLocationRef, EnvironmentRef, InstalledSkill, ProjectInfo } from '@/bindings';

async function loadTargetProjects(environment: EnvironmentRef) {
  const result = await projectWorkspace.execute({
    kind: 'prepareCopyTarget',
    environment,
  });
  if (result.status === 'failed') throw result;
}

function projectsForEnvironment(environment: EnvironmentRef): readonly ProjectInfo[] {
  return projectWorkspace.getSnapshot(environment).projects;
}

async function checkTargetExistence(
  skillName: string,
  environment: EnvironmentRef,
  projectIds: string[],
) {
  const targetProjects = projectsForEnvironment(environment);
  const projectsById = new Map(
    targetProjects.map((project) => [project.binding.id, project]),
  );
  return Promise.all(projectIds.map(async (projectId) => {
    const project = projectsById.get(projectId);
    if (!project) {
      throw new Error(`Copy target project is no longer available: ${projectId}`);
    }
    const context: SkillLocationRef = {
      environment,
      scope: { scope: 'project', project_id: project.binding.id },
    };
    const result = await listSkills(context);
    return {
      projectId,
      hasSkill: result.skills.some((candidate) => candidate.name === skillName),
    };
  }));
}

export function CopyToProjectDialogContainer() {
  const skill = useSkillDialogStore((state) => state.copySkill);
  const context = useSkillDialogStore((state) => state.copyContext);

  if (!skill || !context) return null;

  return (
    <OpenCopyToProjectDialog
      key={`${contextKey(context)}:${skill.canonicalPath}`}
      skill={skill}
      sourceContext={context}
    />
  );
}

function OpenCopyToProjectDialog({
  skill,
  sourceContext,
}: {
  skill: InstalledSkill;
  sourceContext: SkillLocationRef;
}) {
  const environments = useEnvironmentStore((state) => state.environments);
  const projectEnvironments = useMemo(
    () => environments.map((entry) => entry.environment),
    [environments],
  );
  const projectsByEnvironment = useProjectCatalog(projectEnvironments);
  const closeCopyToProject = useSkillDialogStore((state) => state.closeCopyToProject);
  const agentSelection = useCopyAgentSelection(sourceContext, skill.name);

  return (
    <CopyToProjectDialog
      open
      skill={skill}
      sourceContext={sourceContext}
      environments={environments}
      projectsByEnvironment={projectsByEnvironment}
      agentSelection={agentSelection}
      onLoadProjects={loadTargetProjects}
      checkExistence={checkTargetExistence}
      onClose={closeCopyToProject}
      onCopy={executeSkillCopy}
    />
  );
}
