import { listSkills } from '@/hooks/useTauriApi';
import { contextKey, environmentKey } from '@/lib/context';
import { useEnvironmentStore } from '@/stores/environment';
import { useCopyAgentSelection } from '@/hooks/useCopyAgentSelection';
import { useProjectStore } from '@/stores/projects';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { executeSkillCopy } from '@/workflows/skill-copy';
import { CopyToProjectDialog } from './CopyToProjectDialog';
import type { ContextRef, EnvironmentRef, InstalledSkill } from '@/bindings';

async function loadTargetProjects(environment: EnvironmentRef) {
  const environmentState = useEnvironmentStore.getState();
  const environmentInfo = environmentState.environments.find(
    (entry) => environmentKey(entry.environment) === environmentKey(environment),
  );
  if (environment.kind === 'wsl' && environmentInfo?.status !== 'available') {
    await environmentState.connect(environment);
  }
  await useProjectStore.getState().refresh(environment);
}

async function checkTargetExistence(
  skillName: string,
  environment: EnvironmentRef,
  projectIds: string[],
) {
  const targetProjects = useProjectStore.getState()
    .projectsByEnvironment[environmentKey(environment)] ?? [];
  const projectsById = new Map(
    targetProjects.map((project) => [project.binding.id, project]),
  );
  return Promise.all(projectIds.map(async (projectId) => {
    const project = projectsById.get(projectId);
    if (!project) {
      throw new Error(`Copy target project is no longer available: ${projectId}`);
    }
    const context: ContextRef = {
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
  sourceContext: ContextRef;
}) {
  const environments = useEnvironmentStore((state) => state.environments);
  const projectsByEnvironment = useProjectStore((state) => state.projectsByEnvironment);
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
