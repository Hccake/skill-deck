import { getManageAgentSelection } from '@/hooks/useTauriApi';
import { contextKey } from '@/lib/context';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import {
  executeManageAgentChanges,
} from '@/workflows/skill-manage-agents';
import { ManageAgentsDialog } from './ManageAgentsDialog';
import type { SkillLocationRef, InstalledSkill } from '@/bindings';
import type { ManageAgentSelectionSessionRequest } from '@/hooks/useAgentSelectionSession';

async function loadManageAgentSelection(request: ManageAgentSelectionSessionRequest) {
  return getManageAgentSelection(request.context, request.skillName);
}

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
  context: SkillLocationRef;
}) {
  const closeManageAgents = useSkillDialogStore((state) => state.closeManageAgents);
  return (
    <ManageAgentsDialog
      skill={skill}
      context={context}
      loadAgentSelection={loadManageAgentSelection}
      onClose={closeManageAgents}
      onSave={executeManageAgentChanges}
    />
  );
}
