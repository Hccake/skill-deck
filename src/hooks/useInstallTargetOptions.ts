import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AgentId,
  AppError,
  ContextRef,
  InstallTargetInfo,
} from '@/bindings';
import {
  getDefaultTargetAgents,
  listAgentSelectionGroups,
  listAgents,
  listEveInstallTargets,
} from '@/hooks/useTauriApi';
import { agentsForScope } from '@/lib/agents';
import { contextKey, globalContext } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';
import {
  initializeInstallTargetSelection,
  reconcileInstallTargetSelection,
  type InstallTargetFacts,
  type InstallTargetSelection,
} from '@/workflows/install-target-options';
import type { InstallScope } from '@/lib/agentTargets';
import type { WizardState } from '@/components/skills/add-skill/types';

export type InstallTargetOptionsState =
  | { status: 'idle' }
  | { status: 'loading'; inputKey: string }
  | { status: 'ready'; inputKey: string; facts: InstallTargetFacts }
  | { status: 'error'; inputKey: string; error: AppError };

export interface InstallTargetOptionsInput {
  active: boolean;
  context: ContextRef;
  scope: InstallScope;
  preselectedAgents: AgentId[];
  selection: InstallTargetSelection;
  updateState: (updates: Partial<WizardState>) => void;
}

export type InstallTargetOptionsController = InstallTargetOptionsState & {
  retry: () => Promise<void>;
};

function inputKey(context: ContextRef, scope: InstallScope, preselectedAgents: AgentId[]) {
  return JSON.stringify([
    contextKey(context),
    scope,
    [...new Set(preselectedAgents)].sort(),
  ]);
}

function selectionPatch(selection: InstallTargetSelection, facts: InstallTargetFacts) {
  return {
    allAgents: facts.allAgents,
    availableAgentTargets: facts.availableAgentTargets,
    selectedAgents: selection.selectedAgents,
    privateCopyAgents: selection.privateCopyAgents,
    selectedAgentTargets: selection.selectedAgentTargets,
  };
}

export function useInstallTargetOptions({
  active,
  context,
  scope,
  preselectedAgents,
  selection,
  updateState,
}: InstallTargetOptionsInput): InstallTargetOptionsController {
  const key = inputKey(context, scope, preselectedAgents);
  const [state, setState] = useState<InstallTargetOptionsState>({ status: 'idle' });
  const stateRef = useRef<InstallTargetOptionsState>(state);
  const initializedKeyRef = useRef<string | null>(null);
  const loadGenerationRef = useRef(0);
  const previousKeyRef = useRef(key);
  const latestRef = useRef({
    key,
    context,
    scope,
    preselectedAgents,
    selection,
    updateState,
  });

  useEffect(() => {
    latestRef.current = {
      key,
      context,
      scope,
      preselectedAgents,
      selection,
      updateState,
    };
  }, [context, key, preselectedAgents, scope, selection, updateState]);

  const updateControllerState = useCallback((next: InstallTargetOptionsState) => {
    stateRef.current = next;
    setState(next);
  }, []);

  const publishFacts = useCallback((facts: InstallTargetFacts, factsKey: string) => {
    const latest = latestRef.current;
    const previousSelection = latest.selection;
    const nextSelection = initializedKeyRef.current === factsKey
      ? reconcileInstallTargetSelection({
        scope: latest.scope,
        selection: previousSelection,
        facts,
      })
      : initializeInstallTargetSelection({
        scope: latest.scope,
        preselectedAgents: latest.preselectedAgents,
        mode: previousSelection.mode,
        facts,
      });

    initializedKeyRef.current = factsKey;
    latestRef.current.selection = nextSelection;
    latest.updateState(selectionPatch(nextSelection, facts));
    updateControllerState({ status: 'ready', inputKey: factsKey, facts });
  }, [updateControllerState]);

  const load = useCallback(async (force = false) => {
    const latest = latestRef.current;
    const requestedKey = latest.key;
    const currentContext = latest.context;
    const currentScope = latest.scope;
    const current = stateRef.current;
    if (!force && current.status === 'ready' && current.inputKey === requestedKey) {
      return;
    }
    const generation = ++loadGenerationRef.current;

    updateControllerState({ status: 'loading', inputKey: requestedKey });
    try {
      const targetsPromise = currentScope === 'project'
        ? listEveInstallTargets(currentContext)
        : Promise.resolve([] as InstallTargetInfo[]);
      const defaultsPromise = getDefaultTargetAgents(globalContext(currentContext.environment))
        .then((defaults) => ({ defaults, unavailable: false }))
        .catch(() => ({ defaults: null, unavailable: true }));
      const [runtimeSnapshot, selectionGroups, availableAgentTargets, defaultsResult] = await Promise.all([
        listAgents(currentContext),
        listAgentSelectionGroups(currentContext),
        targetsPromise,
        defaultsPromise,
      ]);

      if (generation !== loadGenerationRef.current || requestedKey !== latestRef.current.key) return;
      const facts: InstallTargetFacts = {
        allAgents: agentsForScope(runtimeSnapshot, currentScope),
        selectionGroups: selectionGroups[currentScope],
        availableAgentTargets,
        defaultAgents: defaultsResult.defaults?.[currentScope] ?? null,
        defaultsUnavailable: defaultsResult.unavailable,
      };
      publishFacts(facts, requestedKey);
    } catch (error) {
      if (generation !== loadGenerationRef.current || requestedKey !== latestRef.current.key) return;
      updateControllerState({
        status: 'error',
        inputKey: requestedKey,
        error: toAppError(error),
      });
    }
  }, [publishFacts, updateControllerState]);

  useEffect(() => {
    if (previousKeyRef.current !== key) {
      previousKeyRef.current = key;
      initializedKeyRef.current = null;
      loadGenerationRef.current += 1;
    }
  }, [key]);

  useEffect(() => {
    if (!active) return;
    void load();
  }, [active, key, load]);

  useEffect(() => () => {
    loadGenerationRef.current += 1;
  }, []);

  const retry = useCallback(() => load(true), [load]);

  const visibleState: InstallTargetOptionsState = state.status !== 'idle'
    && state.inputKey !== key
    ? { status: 'idle' }
    : state;

  return {
    ...visibleState,
    retry,
  };
}
