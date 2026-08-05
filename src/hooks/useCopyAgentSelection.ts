import { useCallback, useEffect, useRef, useState } from 'react';
import type { AppError, ContextRef, CopyAgentSelectionSnapshot } from '@/bindings';
import { getCopyAgentSelection } from '@/hooks/useTauriApi';
import { contextKey } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';

export type CopyAgentSelectionState =
  | { status: 'loading' }
  | { status: 'ready'; snapshot: CopyAgentSelectionSnapshot }
  | { status: 'error'; error: AppError };

export function useCopyAgentSelection(
  source: ContextRef,
  skillName: string,
): CopyAgentSelectionState & { retry: () => Promise<void> } {
  const inputKey = `${contextKey(source)}:${skillName}`;
  const [loaded, setLoaded] = useState<{
    inputKey: string;
    state: CopyAgentSelectionState;
  }>({ inputKey, state: { status: 'loading' } });
  const generationRef = useRef(0);
  const latestRef = useRef({ inputKey, source, skillName });

  useEffect(() => {
    latestRef.current = { inputKey, source, skillName };
  }, [inputKey, skillName, source]);

  const request = useCallback(async (current: typeof latestRef.current) => {
    const generation = ++generationRef.current;
    try {
      const snapshot = await getCopyAgentSelection(current.source, current.skillName);
      if (generation !== generationRef.current || current.inputKey !== latestRef.current.inputKey) return;
      setLoaded({ inputKey: current.inputKey, state: { status: 'ready', snapshot } });
    } catch (error) {
      if (generation !== generationRef.current || current.inputKey !== latestRef.current.inputKey) return;
      setLoaded({
        inputKey: current.inputKey,
        state: { status: 'error', error: toAppError(error) },
      });
    }
  }, []);

  useEffect(() => {
    void request(latestRef.current);
    return () => {
      generationRef.current += 1;
    };
  }, [inputKey, request]);

  const retry = useCallback(async () => {
    const current = latestRef.current;
    setLoaded({ inputKey: current.inputKey, state: { status: 'loading' } });
    await request(current);
  }, [request]);

  const state = loaded.inputKey === inputKey ? loaded.state : { status: 'loading' as const };
  return { ...state, retry };
}
