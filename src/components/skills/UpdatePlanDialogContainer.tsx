import { useMemo } from 'react';
import { agentDisplayName, agentId } from '@/lib/agents';
import { contextKey } from '@/lib/context';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { UpdatePlanDialog } from './UpdatePlanDialog';
import type { SkillLocationRef, ResolvedAgent } from '@/bindings';

const EMPTY_AGENTS: ResolvedAgent[] = [];

export function UpdatePlanDialogContainer() {
  const phase = useSkillUpdateWorkflow((state) => state.phase);
  const context = useSkillUpdateWorkflow((state) => state.context);
  const skillNames = useSkillUpdateWorkflow((state) => state.skillNames);

  if (phase === 'closed' || !context) return null;

  return (
    <OpenUpdatePlanDialog
      context={context}
      skillNames={skillNames}
    />
  );
}

function OpenUpdatePlanDialog({
  context,
  skillNames,
}: {
  context: SkillLocationRef;
  skillNames: string[];
}) {
  const agents = useSkillsDataStore((state) => (
    state.snapshots[contextKey(context)]?.agents ?? EMPTY_AGENTS
  ));
  const close = useSkillUpdateWorkflow((state) => state.close);
  const agentDisplayNames = useMemo(
    () => new Map(agents.map((agent) => [agentId(agent), agentDisplayName(agent)])),
    [agents],
  );

  return (
    <UpdatePlanDialog
      open
      context={context}
      skillNames={skillNames}
      agentDisplayNames={agentDisplayNames}
      onOpenChange={(open) => {
        if (!open) close();
      }}
    />
  );
}
