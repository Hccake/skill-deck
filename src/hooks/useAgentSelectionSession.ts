import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  AgentSelectionSnapshot,
  AgentSelectionIntent,
  AgentSelectionSubmission,
  AgentInstallOptionId,
  AppError,
  InstallMode,
  ManageInstallOptionState,
  SkillLocationRef,
} from '@/bindings';
import {
  createAgentSelectionSession,
  hasUserSelectionChanges,
  refreshAgentSelectionSession,
  toggleSelectionGroup,
  toggleInstallOption,
  type AgentSelectionSession,
} from '@/lib/agent-selection-session';
import { toAppError } from '@/utils/to-app-error';
import { contextKey } from '@/lib/context';

export interface AgentSelectionEnvelope {
  selection: AgentSelectionSnapshot;
  optionStates?: ManageInstallOptionState[];
}

export type AgentSelectionSessionRequest =
  | {
    kind: 'install';
    context: SkillLocationRef;
    intent: AgentSelectionIntent;
  }
  | {
    kind: 'copy';
    context: SkillLocationRef;
    skillName: string;
  }
  | {
    kind: 'manage';
    context: SkillLocationRef;
    skillName: string;
  };

export type InstallAgentSelectionSessionRequest = Extract<
  AgentSelectionSessionRequest,
  { kind: 'install' }
>;
export type CopyAgentSelectionSessionRequest = Extract<
  AgentSelectionSessionRequest,
  { kind: 'copy' }
>;
export type ManageAgentSelectionSessionRequest = Extract<
  AgentSelectionSessionRequest,
  { kind: 'manage' }
>;

function sessionRequestKey(request: AgentSelectionSessionRequest): string {
  const subject = request.kind === 'install'
    ? [
        request.intent.wildcardRequested,
        [...new Set(request.intent.explicitAgentIds)].sort(),
      ]
    : request.skillName;
  return JSON.stringify([request.kind, contextKey(request.context), subject]);
}

type SessionState<TSnapshot extends AgentSelectionEnvelope> =
  | { status: 'idle'; requestKey: string }
  | { status: 'loading'; requestKey: string }
  | { status: 'error'; requestKey: string; error: AppError }
  | {
    status: 'ready';
    requestKey: string;
    snapshot: TSnapshot;
    session: AgentSelectionSession;
  };

export type AgentSelectionSessionController<TSnapshot extends AgentSelectionEnvelope> = (
  | { status: 'idle' | 'loading' }
  | { status: 'error'; error: AppError }
  | {
    status: 'ready';
    snapshot: TSnapshot;
    selection: AgentSelectionSnapshot;
    session: AgentSelectionSession;
    optionStates: ManageInstallOptionState[];
    submission: AgentSelectionSubmission;
    requiresReconfirmation: boolean;
    isDirty: boolean;
  }
) & {
  retry: () => Promise<void>;
  setOptionSelected: (optionId: AgentInstallOptionId, selected: boolean) => void;
  setMode: (mode: InstallMode) => void;
  setGroupSelected: (groupId: string, selected: boolean) => void;
  setOtherAgentsExpanded: (expanded: boolean) => void;
  setAdditionalInstallExpanded: (expanded: boolean) => void;
  setGroupExpanded: (groupId: string, expanded: boolean) => void;
  acceptSnapshot: (snapshot: TSnapshot) => void;
  confirmCurrentSelection: () => void;
};

export function useAgentSelectionSession<
  TRequest extends AgentSelectionSessionRequest,
  TSnapshot extends AgentSelectionEnvelope,
>({
  active,
  request: sessionRequest,
  load,
}: {
  active: boolean;
  request: TRequest;
  load: (request: TRequest) => Promise<TSnapshot>;
}): AgentSelectionSessionController<TSnapshot> {
  const requestKey = sessionRequestKey(sessionRequest);
  const [state, setState] = useState<SessionState<TSnapshot>>({
    status: active ? 'loading' : 'idle',
    requestKey,
  });
  const generationRef = useRef(0);
  const loadedKeyRef = useRef<string | null>(null);
  const latestRef = useRef({ requestKey, sessionRequest, load });

  useEffect(() => {
    latestRef.current = { requestKey, sessionRequest, load };
  }, [load, requestKey, sessionRequest]);

  const requestSnapshot = useCallback(async () => {
    const generation = ++generationRef.current;
    const current = latestRef.current;
    loadedKeyRef.current = null;
    setState({ status: 'loading', requestKey: current.requestKey });
    try {
      const snapshot = await current.load(current.sessionRequest);
      if (
        generation !== generationRef.current
        || current.requestKey !== latestRef.current.requestKey
      ) return;
      loadedKeyRef.current = current.requestKey;
      setState({
        status: 'ready',
        requestKey: current.requestKey,
        snapshot,
        session: createAgentSelectionSession(
          snapshot.selection,
          'symlink',
          snapshot.optionStates ?? [],
        ),
      });
    } catch (error) {
      if (
        generation !== generationRef.current
        || current.requestKey !== latestRef.current.requestKey
      ) return;
      setState({
        status: 'error',
        requestKey: current.requestKey,
        error: toAppError(error),
      });
    }
  }, []);

  useEffect(() => {
    if (!active || loadedKeyRef.current === requestKey) return;
    void requestSnapshot();
  }, [active, requestKey, requestSnapshot]);

  useEffect(() => () => {
    generationRef.current += 1;
  }, []);

  const setOptionSelected = useCallback((optionId: AgentInstallOptionId, selected: boolean) => {
    setState((current) => current.status === 'ready'
      ? {
        ...current,
        session: toggleInstallOption(
          current.session,
          current.snapshot.selection,
          optionId,
          selected,
        ),
      }
      : current);
  }, []);

  const setMode = useCallback((mode: InstallMode) => {
    setState((current) => current.status === 'ready'
      ? { ...current, session: { ...current.session, mode } }
      : current);
  }, []);

  const setGroupSelected = useCallback((groupId: string, selected: boolean) => {
    setState((current) => current.status === 'ready'
      ? {
        ...current,
        session: toggleSelectionGroup(
          current.session,
          current.snapshot.selection,
          groupId,
          selected,
        ),
      }
      : current);
  }, []);

  const setOtherAgentsExpanded = useCallback((expanded: boolean) => {
    setState((current) => current.status === 'ready'
      ? { ...current, session: { ...current.session, otherAgentsExpanded: expanded } }
      : current);
  }, []);

  const setAdditionalInstallExpanded = useCallback((expanded: boolean) => {
    setState((current) => current.status === 'ready'
      ? { ...current, session: { ...current.session, additionalInstallExpanded: expanded } }
      : current);
  }, []);

  const setGroupExpanded = useCallback((groupId: string, expanded: boolean) => {
    setState((current) => current.status === 'ready'
      ? {
        ...current,
        session: {
          ...current.session,
          expandedGroupIds: expanded
            ? [...new Set([...current.session.expandedGroupIds, groupId])]
            : current.session.expandedGroupIds.filter((id) => id !== groupId),
        },
      }
      : current);
  }, []);

  const acceptSnapshot = useCallback((snapshot: TSnapshot) => {
    setState((current) => {
      if (current.status !== 'ready') return current;
      if (current.snapshot.selection.revision === snapshot.selection.revision) {
        return current;
      }
      return {
        ...current,
        snapshot,
        session: refreshAgentSelectionSession(
          current.session,
          snapshot.selection,
          snapshot.optionStates ?? [],
        ),
      };
    });
  }, []);

  const confirmCurrentSelection = useCallback(() => {
    setState((current) => current.status === 'ready'
      ? {
        ...current,
        session: { ...current.session, requiresReconfirmation: false },
      }
      : current);
  }, []);

  const actions = useMemo(() => ({
    retry: requestSnapshot,
    setOptionSelected,
    setMode,
    setGroupSelected,
    setOtherAgentsExpanded,
    setAdditionalInstallExpanded,
    setGroupExpanded,
    acceptSnapshot,
    confirmCurrentSelection,
  }), [
    acceptSnapshot,
    confirmCurrentSelection,
    requestSnapshot,
    setAdditionalInstallExpanded,
    setGroupExpanded,
    setGroupSelected,
    setMode,
    setOptionSelected,
    setOtherAgentsExpanded,
  ]);

  return useMemo(() => {
    const visible = state.requestKey === requestKey
      ? state
      : { status: active ? 'loading' as const : 'idle' as const, requestKey };
    if (visible.status !== 'ready') {
      return visible.status === 'error'
        ? { status: 'error' as const, error: visible.error, ...actions }
        : { status: visible.status, ...actions };
    }

    return {
      status: 'ready' as const,
      snapshot: visible.snapshot,
      selection: visible.snapshot.selection,
      session: visible.session,
      optionStates: visible.snapshot.optionStates ?? [],
      submission: {
        revision: visible.snapshot.selection.revision,
        selectedOptionIds: visible.session.selectedOptionIds,
        requestedMode: visible.session.mode,
      },
      requiresReconfirmation: visible.session.requiresReconfirmation,
      isDirty: hasUserSelectionChanges(visible.session, visible.snapshot.selection),
      ...actions,
    };
  }, [actions, active, requestKey, state]);
}
