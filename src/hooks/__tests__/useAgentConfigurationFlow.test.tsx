/* @vitest-environment jsdom */

import '@/test-utils';
import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useAgentConfigurationFlow } from '../useAgentConfigurationFlow';

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  request: vi.fn(),
  listAgents: vi.fn(),
  completedCallback: null as null | ((event: {
    payload: { agentId: string; outcome: 'saved' | 'cancelled' };
  }) => void),
}));

vi.mock('@/bindings', () => ({
  events: {
    agentConfigurationCompletedEvent: {
      listen: (callback: typeof mocks.completedCallback) => {
        mocks.completedCallback = callback;
        return mocks.listen(callback);
      },
    },
  },
}));

vi.mock('@/hooks/useTauriApi', () => ({
  requestAgentConfiguration: (agentId: string) => mocks.request(agentId),
  listAgents: (context: unknown) => mocks.listAgents(context),
}));

const context = { environment: { kind: 'host' }, scope: { scope: 'global' } } as const;
const runtime = {
  registryRevision: 'registry-2', environmentRevision: 'environment-1',
  environment: context.environment, availability: 'available', projectPath: null,
  agents: { 'private-agent': { definition: { id: 'private-agent' } } },
};

describe('useAgentConfigurationFlow', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.completedCallback = null;
    mocks.listen.mockResolvedValue(() => undefined);
    mocks.request.mockResolvedValue(undefined);
    mocks.listAgents.mockResolvedValue(runtime);
  });

  it('listens before requesting and selects the Agent after a saved completion', async () => {
    const onSaved = vi.fn();
    const { result } = renderHook(() => useAgentConfigurationFlow({ context, onSaved }));

    await waitFor(() => expect(mocks.listen).toHaveBeenCalled());
    await act(async () => result.current.configure('private-agent'));
    expect(mocks.listen.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.request.mock.invocationCallOrder[0],
    );

    await act(async () => mocks.completedCallback?.({
      payload: { agentId: 'private-agent', outcome: 'saved' },
    }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(runtime, 'private-agent'));
    expect(result.current.configurationResult).toBe('saved');
    expect(result.current.configuringAgentId).toBeNull();
  });

  it('clears the pending Agent when configuration is cancelled', async () => {
    const onSaved = vi.fn();
    const { result } = renderHook(() => useAgentConfigurationFlow({ context, onSaved }));
    await waitFor(() => expect(mocks.listen).toHaveBeenCalled());
    await act(async () => result.current.configure('private-agent'));

    act(() => mocks.completedCallback?.({
      payload: { agentId: 'private-agent', outcome: 'cancelled' },
    }));

    expect(onSaved).not.toHaveBeenCalled();
    expect(result.current.configurationResult).toBe('cancelled');
    expect(result.current.configuringAgentId).toBeNull();
  });

  it('ignores completion events for a different Agent', async () => {
    const { result } = renderHook(() => useAgentConfigurationFlow({ context, onSaved: vi.fn() }));
    await waitFor(() => expect(mocks.listen).toHaveBeenCalled());
    await act(async () => result.current.configure('private-agent'));

    act(() => mocks.completedCallback?.({
      payload: { agentId: 'other-agent', outcome: 'saved' },
    }));

    expect(result.current.configuringAgentId).toBe('private-agent');
    expect(mocks.listAgents).not.toHaveBeenCalled();
  });

  it('refreshes the Registry on focus and accepts a saved Agent when an event was missed', async () => {
    const onSaved = vi.fn();
    const { result } = renderHook(() => useAgentConfigurationFlow({ context, onSaved }));
    await waitFor(() => expect(mocks.listen).toHaveBeenCalled());
    await act(async () => result.current.configure('private-agent'));

    act(() => window.dispatchEvent(new Event('focus')));

    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(runtime, 'private-agent'));
    expect(result.current.configurationResult).toBe('saved');
  });

  it('keeps focus refresh available when event registration fails', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const onSaved = vi.fn();
    mocks.listen.mockRejectedValue(new Error('listener unavailable'));
    const { result } = renderHook(() => useAgentConfigurationFlow({ context, onSaved }));
    await waitFor(() => expect(consoleError).toHaveBeenCalledTimes(1));
    await act(async () => result.current.configure('private-agent'));

    act(() => window.dispatchEvent(new Event('focus')));

    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(runtime, 'private-agent'));
    consoleError.mockRestore();
  });

  it('reports request failure without leaking an unhandled rejection to the caller', async () => {
    mocks.request.mockRejectedValue(new Error('main window unavailable'));
    const { result } = renderHook(() => useAgentConfigurationFlow({ context, onSaved: vi.fn() }));
    await waitFor(() => expect(mocks.listen).toHaveBeenCalled());

    await act(async () => expect(result.current.configure('private-agent')).resolves.toBeUndefined());

    expect(result.current.configuringAgentId).toBeNull();
    expect(result.current.configurationResult).toBe('failed');
  });
});
