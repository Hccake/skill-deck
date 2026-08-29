import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { AppError, EnvironmentRef, FetchResult } from '@/bindings';
import { environmentKey } from '@/lib/context';
import {
  resolveSourceSelection,
  type ResolvedSourceSelection,
} from '@/lib/source-discovery/source-selection';
import { discoverSkillSource } from '@/hooks/useTauriApi';
import { toAppError } from '@/utils/to-app-error';

export interface CloneProgress {
  phase: 'connecting' | 'cloning' | 'done' | 'error';
  elapsed_secs: number;
  timeout_secs: number;
  message: string | null;
}

interface CloneProgressEvent extends CloneProgress {
  operation_id: string;
}

export interface SourceDiscoveryOutcome {
  result: FetchResult;
  selection: ResolvedSourceSelection;
}

export interface SourceDiscoveryController {
  status: 'idle' | 'loading' | 'error' | 'success';
  operationId: string | null;
  sourceInput: string;
  result: FetchResult | null;
  selection: ResolvedSourceSelection | null;
  error: AppError | null;
  cloneProgress: CloneProgress | null;
  discover: (sourceInput: string) => Promise<SourceDiscoveryOutcome | null>;
  retry: () => Promise<SourceDiscoveryOutcome | null>;
  reset: () => void;
}

interface DiscoveryState {
  status: SourceDiscoveryController['status'];
  operationId: string | null;
  sourceInput: string;
  result: FetchResult | null;
  selection: ResolvedSourceSelection | null;
  error: AppError | null;
  cloneProgress: CloneProgress | null;
}

const INITIAL_STATE: DiscoveryState = {
  status: 'idle',
  operationId: null,
  sourceInput: '',
  result: null,
  selection: null,
  error: null,
  cloneProgress: null,
};

export function useSourceDiscovery(environment: EnvironmentRef): SourceDiscoveryController {
  const [state, setState] = useState<DiscoveryState>(INITIAL_STATE);
  const activeOperationIdRef = useRef<string | null>(null);
  const lastSourceInputRef = useRef('');
  const mountedRef = useRef(true);
  const currentEnvironmentKey = environmentKey(environment);
  const previousEnvironmentKeyRef = useRef(currentEnvironmentKey);

  const reset = useCallback(() => {
    activeOperationIdRef.current = null;
    lastSourceInputRef.current = '';
    setState(INITIAL_STATE);
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    const unlisten = listen<CloneProgressEvent>('clone-progress', (event) => {
      if (event.payload.operation_id !== activeOperationIdRef.current) return;
      setState((current) => ({
        ...current,
        cloneProgress: {
          phase: event.payload.phase,
          elapsed_secs: event.payload.elapsed_secs,
          timeout_secs: event.payload.timeout_secs,
          message: event.payload.message,
        },
      }));
    }).catch((error) => {
      console.error('Failed to monitor source discovery progress:', error);
      return () => undefined;
    });

    return () => {
      mountedRef.current = false;
      activeOperationIdRef.current = null;
      void unlisten.then((stopListening) => stopListening());
    };
  }, []);

  useEffect(() => {
    if (previousEnvironmentKeyRef.current === currentEnvironmentKey) return;
    previousEnvironmentKeyRef.current = currentEnvironmentKey;
    reset();
  }, [currentEnvironmentKey, reset]);

  const discover = useCallback(async (sourceInput: string) => {
    const requestSelection = resolveSourceSelection(sourceInput, {
      skills: [],
      skillFilter: null,
    });
    const operationId = crypto.randomUUID();
    activeOperationIdRef.current = operationId;
    lastSourceInputRef.current = sourceInput;
    setState({
      status: 'loading',
      operationId,
      sourceInput,
      result: null,
      selection: null,
      error: null,
      cloneProgress: null,
    });

    try {
      const result = await discoverSkillSource(
        environment,
        requestSelection.source,
        operationId,
        requestSelection.sourceSelectionIntent,
      );
      if (!mountedRef.current || activeOperationIdRef.current !== operationId) return null;

      if (result.skills.length === 0) {
        setState((current) => ({
          ...current,
          status: 'error',
          error: { kind: 'noSkillsFound' },
          result: null,
          selection: null,
        }));
        return null;
      }

      const selection = resolveSourceSelection(sourceInput, result);
      const outcome = { result, selection };
      setState((current) => ({
        ...current,
        status: 'success',
        result,
        selection,
        error: null,
      }));
      return outcome;
    } catch (error) {
      if (!mountedRef.current || activeOperationIdRef.current !== operationId) return null;
      setState((current) => ({
        ...current,
        status: 'error',
        result: null,
        selection: null,
        error: toAppError(error),
      }));
      return null;
    }
  }, [environment]);

  const retry = useCallback(() => {
    const sourceInput = lastSourceInputRef.current;
    if (!sourceInput) return Promise.resolve(null);
    return discover(sourceInput);
  }, [discover]);

  return {
    ...state,
    discover,
    retry,
    reset,
  };
}
