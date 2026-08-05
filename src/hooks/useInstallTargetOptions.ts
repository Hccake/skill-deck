import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AgentId,
  AppError,
  ContextRef,
  InstallAgentSelectionSnapshot,
} from '@/bindings';
import { getInstallAgentSelection } from '@/hooks/useTauriApi';
import { contextKey } from '@/lib/context';
import {
  createAgentSelectionSession,
  refreshAgentSelectionSession,
} from '@/lib/agent-selection-session';
import { toAppError } from '@/utils/to-app-error';
import type { WizardState } from '@/components/skills/add-skill/types';

export type InstallTargetOptionsState =
  | { status: 'idle' }
  | { status: 'loading'; inputKey: string }
  | { status: 'ready'; inputKey: string; snapshot: InstallAgentSelectionSnapshot }
  | { status: 'error'; inputKey: string; error: AppError };

export interface InstallTargetOptionsInput {
  active: boolean;
  context: ContextRef;
  preselectedAgents: AgentId[];
  snapshot: InstallAgentSelectionSnapshot | null;
  selectedItemIds: string[];
  mode: WizardState['mode'];
  updateState: (updates: Partial<WizardState>) => void;
}

export type InstallTargetOptionsController = InstallTargetOptionsState & {
  retry: () => Promise<void>;
};

function inputKey(context: ContextRef, preselectedAgents: AgentId[]) {
  return JSON.stringify([
    contextKey(context),
    [...new Set(preselectedAgents)].sort(),
  ]);
}

export function useInstallTargetOptions({
  active,
  context,
  preselectedAgents,
  snapshot,
  selectedItemIds,
  mode,
  updateState,
}: InstallTargetOptionsInput): InstallTargetOptionsController {
  const key = inputKey(context, preselectedAgents);
  const [state, setState] = useState<InstallTargetOptionsState>({ status: 'idle' });
  const generationRef = useRef(0);
  const initializedKeyRef = useRef<string | null>(null);
  const loadedKeyRef = useRef<string | null>(null);
  const latestRef = useRef({ key, context, preselectedAgents, snapshot, selectedItemIds, mode, updateState });

  useEffect(() => {
    latestRef.current = { key, context, preselectedAgents, snapshot, selectedItemIds, mode, updateState };
  }, [context, key, mode, preselectedAgents, selectedItemIds, snapshot, updateState]);

  const load = useCallback(async () => {
    const generation = ++generationRef.current;
    const current = latestRef.current;
    setState({ status: 'loading', inputKey: current.key });
    try {
      const nextSnapshot = await getInstallAgentSelection(
        current.context,
        current.preselectedAgents,
      );
      if (generation !== generationRef.current || current.key !== latestRef.current.key) return;
      const session = initializedKeyRef.current === current.key && current.snapshot
        ? refreshAgentSelectionSession({
          ...createAgentSelectionSession(current.snapshot.selection),
          selectedItemIds: current.selectedItemIds,
          mode: current.mode,
        }, nextSnapshot.selection)
        : createAgentSelectionSession(nextSnapshot.selection);
      initializedKeyRef.current = current.key;
      loadedKeyRef.current = current.key;
      current.updateState({
        agentSelectionSnapshot: nextSnapshot,
        selectedAgentItemIds: session.selectedItemIds,
        otherAgentsExpanded: session.otherAgentsExpanded,
        additionalAgentsExpanded: session.additionalInstallExpanded,
        expandedAgentGroupIds: session.expandedGroupIds,
        selectionRequiresReconfirmation: false,
      });
      setState({ status: 'ready', inputKey: current.key, snapshot: nextSnapshot });
    } catch (error) {
      if (generation !== generationRef.current || current.key !== latestRef.current.key) return;
      setState({ status: 'error', inputKey: current.key, error: toAppError(error) });
    }
  }, []);

  useEffect(() => {
    if (!active || loadedKeyRef.current === key) return;
    void load();
  }, [active, key, load]);

  useEffect(() => () => {
    generationRef.current += 1;
  }, []);

  const visible = state.status !== 'idle' && state.inputKey !== key
    ? { status: 'idle' as const }
    : state;
  return { ...visible, retry: load };
}
