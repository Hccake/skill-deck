import { useCallback } from 'react';
import { contextKey } from '@/lib/context';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import {
  executeManageAgentChanges,
  openManageAgentChanges,
} from '@/workflows/skill-manage-agents';
import { ManageAgentsDialog } from './ManageAgentsDialog';
import type { ContextRef, InstalledSkill, ResolvedAgent } from '@/bindings';

const EMPTY_AGENTS: ResolvedAgent[] = [];

export function ManageAgentsDialogContainer() {
  const skill = useSkillDialogStore((state) => state.manageAgentsSkill);
  const context = useSkillDialogStore((state) => state.manageAgentsContext);

  if (!skill || !context) return null;

  return (
    <OpenManageAgentsDialog
      key={`${contextKey(context)}:${skill.canonicalPath}`}
      skill={skill}
      context={context}
    />
  );
}

function OpenManageAgentsDialog({
  skill,
  context,
}: {
  skill: InstalledSkill;
  context: ContextRef;
}) {
  const agentDetails = useSkillDialogStore((state) => state.manageAgentDetails);
  const loadingAgentDetails = useSkillDialogStore((state) => state.loadingManageAgentDetails);
  const projectPath = useSkillDialogStore((state) => state.manageAgentsProjectPath);
  const closeManageAgents = useSkillDialogStore((state) => state.closeManageAgents);
  const previewFailed = !loadingAgentDetails && agentDetails === null;
  const allAgents = agentDetails?.availableAgents ?? EMPTY_AGENTS;

  const retryPreview = useCallback(() => {
    void openManageAgentChanges(skill, context, projectPath);
  }, [context, projectPath, skill]);

  return (
    <ManageAgentsDialog
      skill={skill}
      scope={context.scope.scope}
      allAgents={allAgents}
      agentDetails={agentDetails}
      loadingAgentDetails={loadingAgentDetails}
      previewFailed={previewFailed}
      onRetry={retryPreview}
      onClose={closeManageAgents}
      onSave={executeManageAgentChanges}
    />
  );
}
