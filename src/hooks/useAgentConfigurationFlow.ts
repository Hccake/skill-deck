import { useCallback, useEffect, useRef, useState } from 'react';
import { events } from '@/bindings';
import { listAgents, requestAgentConfiguration } from '@/hooks/useTauriApi';
import type { AgentId, AgentRuntimeSnapshot, ContextRef } from '@/bindings';

export type AgentConfigurationTerminalResult = 'saved' | 'cancelled' | 'failed';

export function useAgentConfigurationFlow({
  context,
  onSaved,
}: {
  context: ContextRef;
  onSaved: (snapshot: AgentRuntimeSnapshot, agentId: AgentId) => void;
}) {
  const [configuringAgentId, setConfiguringAgentId] = useState<AgentId | null>(null);
  const [configurationResult, setConfigurationResult] = useState<AgentConfigurationTerminalResult | null>(null);
  const configuringAgentRef = useRef<AgentId | null>(null);
  const requestGeneration = useRef(0);
  const onSavedRef = useRef(onSaved);
  const contextRef = useRef(context);

  useEffect(() => {
    onSavedRef.current = onSaved;
    contextRef.current = context;
  }, [context, onSaved]);

  const finish = useCallback((agentId: AgentId, result: AgentConfigurationTerminalResult) => {
    if (configuringAgentRef.current !== agentId) return;
    configuringAgentRef.current = null;
    requestGeneration.current += 1;
    setConfiguringAgentId(null);
    setConfigurationResult(result);
  }, []);

  const refreshConfiguredAgent = useCallback(async (agentId: AgentId, failWhenMissing: boolean) => {
    const generation = requestGeneration.current;
    try {
      const snapshot = await listAgents(contextRef.current);
      if (generation !== requestGeneration.current || configuringAgentRef.current !== agentId) return;
      if (snapshot.agents[agentId]) {
        onSavedRef.current(snapshot, agentId);
        finish(agentId, 'saved');
      } else if (failWhenMissing) {
        finish(agentId, 'failed');
      }
    } catch {
      if (generation === requestGeneration.current && failWhenMissing) finish(agentId, 'failed');
    }
  }, [finish]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void events.agentConfigurationCompletedEvent.listen((event) => {
      if (disposed || configuringAgentRef.current !== event.payload.agentId) return;
      if (event.payload.outcome === 'saved') {
        void refreshConfiguredAgent(event.payload.agentId, true);
      } else {
        finish(event.payload.agentId, 'cancelled');
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch((error) => {
      if (!disposed) console.error('Failed to monitor Agent configuration completion:', error);
    });

    const onFocus = () => {
      const agentId = configuringAgentRef.current;
      if (agentId) void refreshConfiguredAgent(agentId, false);
    };
    window.addEventListener('focus', onFocus);
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener('focus', onFocus);
    };
  }, [finish, refreshConfiguredAgent]);

  const configure = useCallback(async (agentId: AgentId) => {
    requestGeneration.current += 1;
    configuringAgentRef.current = agentId;
    setConfigurationResult(null);
    setConfiguringAgentId(agentId);
    try {
      await requestAgentConfiguration(agentId);
    } catch {
      finish(agentId, 'failed');
    }
  }, [finish]);

  return {
    configuringAgentId,
    configurationResult,
    clearConfigurationResult: () => setConfigurationResult(null),
    configure,
  };
}
