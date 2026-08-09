import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { useAgentSelectionSession } from '@/hooks/useAgentSelectionSession';

const nativeRequest = {
  kind: 'install' as const,
  context: { environment: { kind: 'native' as const }, scope: { scope: 'global' as const } },
  explicitAgentIds: [],
};

describe('useAgentSelectionSession', () => {
  it('loads its session snapshot and exposes a submission through its public interface', async () => {
    const snapshot = {
      selection: makeAgentSelectionSnapshot({
        revision: 'revision-1',
        installOptions: [{
          id: 'claude',
          kind: 'standardDirectory' as const,
          agentIds: ['claude'],
          displayName: 'Claude',
          path: '/agents/claude',
          groupId: null,
          selectable: true,
          modeConstraint: 'userSelectable' as const,
          disabledReason: null,
        }],
        initialSelectedOptionIds: ['claude'],
        userModeOptionIds: ['claude'],
      }),
    };
    const load = vi.fn().mockResolvedValue(snapshot);

    const { result } = renderHook(() => useAgentSelectionSession({
      active: true,
      request: nativeRequest,
      load,
    }));

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(load).toHaveBeenCalledTimes(1);
    expect(result.current.status === 'ready' && result.current.submission).toEqual({
      revision: 'revision-1',
      selectedOptionIds: ['claude'],
      requestedMode: 'symlink',
    });
  });

  it('updates the submission when the user changes an option and installation mode', async () => {
    const snapshot = {
      selection: makeAgentSelectionSnapshot({
        installOptions: [{
          id: 'claude',
          kind: 'standardDirectory' as const,
          agentIds: ['claude'],
          displayName: 'Claude',
          path: '/agents/claude',
          groupId: null,
          selectable: true,
          modeConstraint: 'userSelectable' as const,
          disabledReason: null,
        }],
        initialSelectedOptionIds: ['claude'],
        userModeOptionIds: ['claude'],
      }),
    };
    const { result } = renderHook(() => useAgentSelectionSession({
      active: true,
      request: nativeRequest,
      load: () => Promise.resolve(snapshot),
    }));
    await waitFor(() => expect(result.current.status).toBe('ready'));

    act(() => {
      if (result.current.status !== 'ready') throw new Error('selection not ready');
      result.current.setOptionSelected('claude', false);
      result.current.setMode('copy');
    });

    expect(result.current.status === 'ready' && result.current.submission).toEqual({
      revision: 'selection-revision-1',
      selectedOptionIds: [],
      requestedMode: 'copy',
    });
  });

  it('merges a newer snapshot and requires confirmation before submission', async () => {
    const option = {
      id: 'existing',
      kind: 'standardDirectory' as const,
      agentIds: ['existing'],
      displayName: 'Existing',
      path: '/agents/existing',
      groupId: null,
      selectable: true,
      modeConstraint: 'userSelectable' as const,
      disabledReason: null,
    };
    const initial = {
      selection: makeAgentSelectionSnapshot({
        revision: 'revision-1',
        installOptions: [option],
        initialSelectedOptionIds: ['existing'],
      }),
    };
    const latest = {
      selection: makeAgentSelectionSnapshot({
        revision: 'revision-2',
        installOptions: [
          option,
          { ...option, id: 'new-default', displayName: 'New default' },
        ],
        initialSelectedOptionIds: ['existing', 'new-default'],
      }),
    };
    const { result } = renderHook(() => useAgentSelectionSession({
      active: true,
      request: nativeRequest,
      load: () => Promise.resolve(initial),
    }));
    await waitFor(() => expect(result.current.status).toBe('ready'));

    act(() => {
      if (result.current.status !== 'ready') throw new Error('selection not ready');
      result.current.setOptionSelected('existing', false);
      result.current.acceptSnapshot(latest);
    });

    expect(result.current.status === 'ready' && result.current.submission).toEqual({
      revision: 'revision-2',
      selectedOptionIds: ['new-default'],
      requestedMode: 'symlink',
    });
    expect(result.current.status === 'ready' && result.current.requiresReconfirmation).toBe(true);

    act(() => result.current.confirmCurrentSelection());
    expect(result.current.status === 'ready' && result.current.requiresReconfirmation).toBe(false);
  });

  it('retries a failed snapshot request', async () => {
    const snapshot = { selection: makeAgentSelectionSnapshot() };
    const load = vi.fn()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce(snapshot);
    const { result } = renderHook(() => useAgentSelectionSession({
      active: true,
      request: nativeRequest,
      load,
    }));
    await waitFor(() => expect(result.current.status).toBe('error'));

    await act(async () => result.current.retry());

    expect(result.current.status).toBe('ready');
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('ignores an earlier response after a retry has completed', async () => {
    let resolveFirst!: (value: { selection: ReturnType<typeof makeAgentSelectionSnapshot> }) => void;
    const first = new Promise<{ selection: ReturnType<typeof makeAgentSelectionSnapshot> }>(
      (resolve) => { resolveFirst = resolve; },
    );
    const latest = {
      selection: makeAgentSelectionSnapshot({ revision: 'latest-revision' }),
    };
    const load = vi.fn()
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce(latest);
    const { result } = renderHook(() => useAgentSelectionSession({
      active: true,
      request: nativeRequest,
      load,
    }));

    await act(async () => result.current.retry());
    await waitFor(() => expect(result.current.status).toBe('ready'));
    await act(async () => resolveFirst({
      selection: makeAgentSelectionSnapshot({ revision: 'earlier-revision' }),
    }));

    expect(result.current.status === 'ready' && result.current.selection.revision)
      .toBe('latest-revision');
  });

  it('loads a new session and ignores the earlier response when the request changes', async () => {
    let resolveNative!: (value: { selection: ReturnType<typeof makeAgentSelectionSnapshot> }) => void;
    const native = new Promise<{ selection: ReturnType<typeof makeAgentSelectionSnapshot> }>(
      (resolve) => { resolveNative = resolve; },
    );
    const wslRequest = {
      ...nativeRequest,
      context: {
        ...nativeRequest.context,
        environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      },
    };
    const load = vi.fn((request: typeof nativeRequest | typeof wslRequest) => (
      request.context.environment.kind === 'native'
        ? native
        : Promise.resolve({
          selection: makeAgentSelectionSnapshot({ revision: 'wsl-revision' }),
        })
    ));
    const { result, rerender } = renderHook(
      ({ request }) => useAgentSelectionSession({ active: true, request, load }),
      { initialProps: { request: nativeRequest as typeof nativeRequest | typeof wslRequest } },
    );

    rerender({ request: wslRequest });
    await waitFor(() => expect(result.current.status).toBe('ready'));
    await act(async () => resolveNative({
      selection: makeAgentSelectionSnapshot({ revision: 'native-revision' }),
    }));

    expect(result.current.status === 'ready' && result.current.selection.revision)
      .toBe('wsl-revision');
  });

  it('reloads the original session when a pending request switches back to it', async () => {
    let resolveWsl!: (value: { selection: ReturnType<typeof makeAgentSelectionSnapshot> }) => void;
    const wsl = new Promise<{ selection: ReturnType<typeof makeAgentSelectionSnapshot> }>(
      (resolve) => { resolveWsl = resolve; },
    );
    const wslRequest = {
      ...nativeRequest,
      context: {
        ...nativeRequest.context,
        environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      },
    };
    const load = vi.fn()
      .mockResolvedValueOnce({
        selection: makeAgentSelectionSnapshot({ revision: 'native-initial' }),
      })
      .mockReturnValueOnce(wsl)
      .mockResolvedValueOnce({
        selection: makeAgentSelectionSnapshot({ revision: 'native-reloaded' }),
      });
    const { result, rerender } = renderHook(
      ({ request }) => useAgentSelectionSession({ active: true, request, load }),
      { initialProps: { request: nativeRequest as typeof nativeRequest | typeof wslRequest } },
    );
    await waitFor(() => expect(result.current.status).toBe('ready'));

    rerender({ request: wslRequest });
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    rerender({ request: nativeRequest });

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(result.current.status === 'ready' && result.current.selection.revision)
      .toBe('native-reloaded');
    expect(load).toHaveBeenCalledTimes(3);

    await act(async () => resolveWsl({
      selection: makeAgentSelectionSnapshot({ revision: 'wsl-stale' }),
    }));
    expect(result.current.status === 'ready' && result.current.selection.revision)
      .toBe('native-reloaded');
  });

  it('retains the current session while loading is inactive', async () => {
    const load = vi.fn().mockResolvedValue({
      selection: makeAgentSelectionSnapshot({ revision: 'current-session' }),
    });
    const { result, rerender } = renderHook(
      ({ active }) => useAgentSelectionSession({
        active,
        request: nativeRequest,
        load,
      }),
      { initialProps: { active: true } },
    );
    await waitFor(() => expect(result.current.status).toBe('ready'));

    rerender({ active: false });
    expect(result.current.status === 'ready' && result.current.selection.revision)
      .toBe('current-session');
    rerender({ active: true });

    expect(result.current.status === 'ready' && result.current.selection.revision)
      .toBe('current-session');
    expect(load).toHaveBeenCalledOnce();
  });
});
