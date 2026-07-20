/* @vitest-environment jsdom */

import '@/test-utils';
import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { makeAgentRuntimeSnapshot, makeResolvedAgent, makeResolvedAgentScope } from '@/test-utils';
import type { WizardState } from '@/components/skills/add-skill/types';
import type { ContextRef, ResolvedAgent } from '@/bindings';
import { useInstallTargetOptions } from '../useInstallTargetOptions';

const mocks = vi.hoisted(() => ({
  listAgents: vi.fn(),
  listGroups: vi.fn(),
  listTargets: vi.fn(),
  getDefaults: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listAgents: (context: unknown) => mocks.listAgents(context),
  listAgentSelectionGroups: (context: unknown) => mocks.listGroups(context),
  listEveInstallTargets: (context: unknown) => mocks.listTargets(context),
  getDefaultTargetAgents: (context: unknown) => mocks.getDefaults(context),
}));

const context: ContextRef = {
  environment: { kind: 'host' },
  scope: { scope: 'global' },
} as const;

function privateAgent(id = 'private-agent') {
  return makeResolvedAgent({
    id,
    global: makeResolvedAgentScope({
      readsShared: false,
      privatePath: `~/.${id}/skills`,
      readPaths: [`~/.${id}/skills`],
    }),
  });
}

function createSelectionState(): Pick<
  WizardState,
  'selectedAgents' | 'privateCopyAgents' | 'selectedAgentTargets' | 'mode'
> {
  return {
    selectedAgents: [],
    privateCopyAgents: [],
    selectedAgentTargets: [],
    mode: 'symlink',
  };
}

describe('useInstallTargetOptions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listAgents.mockResolvedValue(makeAgentRuntimeSnapshot([privateAgent()]));
    mocks.listGroups.mockResolvedValue({ global: [], project: [] });
    mocks.listTargets.mockResolvedValue([]);
    mocks.getDefaults.mockResolvedValue({
      global: ['private-agent'],
      project: [],
    });
  });

  it('loads one fact snapshot for the current input and reuses it after Options remounts', async () => {
    let selection = createSelectionState();
    const updateState = vi.fn((update: Partial<WizardState> | ((state: WizardState) => Partial<WizardState>)) => {
      const patch = typeof update === 'function'
        ? update(selection as WizardState)
        : update;
      selection = { ...selection, ...patch };
    });
    const { result, rerender } = renderHook(
      ({ active }) => useInstallTargetOptions({
        active,
        context,
        scope: 'global',
        preselectedAgents: [],
        selection,
        updateState,
      }),
      { initialProps: { active: true } },
    );

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(selection.selectedAgents).toEqual(['private-agent']);
    rerender({ active: false });
    rerender({ active: true });

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(mocks.listAgents).toHaveBeenCalledTimes(1);
    expect(mocks.listGroups).toHaveBeenCalledTimes(1);
    expect(mocks.getDefaults).toHaveBeenCalledTimes(1);
  });

  it('does not publish a late response from the previous Context', async () => {
    let resolveHost!: (value: ReturnType<typeof makeAgentRuntimeSnapshot>) => void;
    mocks.listAgents
      .mockReturnValueOnce(new Promise((resolve) => { resolveHost = resolve; }))
      .mockResolvedValueOnce(makeAgentRuntimeSnapshot([privateAgent('wsl-agent')]));
    const updateState = vi.fn();
    const { result, rerender } = renderHook(
      ({ targetContext }) => useInstallTargetOptions({
        active: true,
        context: targetContext,
        scope: 'global',
        preselectedAgents: [],
        selection: createSelectionState(),
        updateState,
      }),
      { initialProps: { targetContext: context } },
    );
    const wslContext = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    } as const;

    rerender({ targetContext: wslContext });
    await waitFor(() => expect(result.current.status).toBe('ready'));
    await act(async () => resolveHost(makeAgentRuntimeSnapshot([privateAgent('host-agent')])));

    expect(updateState.mock.calls.flatMap(([update]) => {
      if (typeof update !== 'function') return [];
      const patch = update(createSelectionState() as WizardState);
      return patch.allAgents?.map((agent: ResolvedAgent) => agent.definition.id) ?? [];
    })).not.toContain('host-agent');
  });

  it('keeps required fact failures retryable instead of converting them to empty facts', async () => {
    mocks.listTargets.mockRejectedValueOnce(new Error('project unavailable'));
    const projectContext = {
      environment: { kind: 'host' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;
    const { result } = renderHook(() => useInstallTargetOptions({
      active: true,
      context: projectContext,
      scope: 'project',
      preselectedAgents: [],
      selection: createSelectionState(),
      updateState: vi.fn(),
    }));

    await waitFor(() => expect(result.current.status).toBe('error'));
    await act(async () => result.current.retry());
    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(mocks.listTargets).toHaveBeenCalledTimes(2);
  });

  it('marks saved defaults as unavailable without failing required facts', async () => {
    mocks.getDefaults.mockRejectedValue(new Error('lock unavailable'));
    const { result } = renderHook(() => useInstallTargetOptions({
      active: true,
      context,
      scope: 'global',
      preselectedAgents: [],
      selection: createSelectionState(),
      updateState: vi.fn(),
    }));

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(result.current.status === 'ready' && result.current.facts.defaultsUnavailable).toBe(true);
  });

  it('accepts a configured Agent snapshot without loading the Registry twice', async () => {
    mocks.listAgents.mockResolvedValue(makeAgentRuntimeSnapshot([]));
    let selection = createSelectionState();
    const updateState = vi.fn((patch: Partial<WizardState>) => {
      selection = { ...selection, ...patch };
    });
    const { result } = renderHook(() => useInstallTargetOptions({
      active: true,
      context,
      scope: 'global',
      preselectedAgents: ['configured-agent'],
      selection,
      updateState,
    }));
    await waitFor(() => expect(result.current.status).toBe('ready'));

    act(() => result.current.acceptConfiguredAgent(
      makeAgentRuntimeSnapshot([privateAgent('configured-agent')]),
      'configured-agent',
    ));

    expect(selection.selectedAgents).toEqual(['configured-agent']);
    expect(mocks.listAgents).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(mocks.listGroups).toHaveBeenCalledTimes(2));
  });

  it('keeps the configured Agent group refresh when Options becomes inactive', async () => {
    let resolveRefreshedGroups!: (groups: {
      global: Array<{ groupId: string; agentIds: string[] }>;
      project: Array<{ groupId: string; agentIds: string[] }>;
    }) => void;
    mocks.listAgents.mockResolvedValue(makeAgentRuntimeSnapshot([]));
    mocks.listGroups
      .mockResolvedValueOnce({ global: [], project: [] })
      .mockReturnValueOnce(new Promise((resolve) => {
        resolveRefreshedGroups = resolve;
      }));
    const { result, rerender } = renderHook(
      ({ active }) => useInstallTargetOptions({
        active,
        context,
        scope: 'global',
        preselectedAgents: ['configured-agent'],
        selection: createSelectionState(),
        updateState: vi.fn(),
      }),
      { initialProps: { active: true } },
    );
    await waitFor(() => expect(result.current.status).toBe('ready'));

    act(() => result.current.acceptConfiguredAgent(
      makeAgentRuntimeSnapshot([privateAgent('configured-agent')]),
      'configured-agent',
    ));
    rerender({ active: false });
    await act(async () => resolveRefreshedGroups({
      global: [{ groupId: 'configured-group', agentIds: ['configured-agent'] }],
      project: [],
    }));
    rerender({ active: true });

    await waitFor(() => expect(
      result.current.status === 'ready' ? result.current.facts.selectionGroups : [],
    ).toEqual([
      { groupId: 'configured-group', agentIds: ['configured-agent'] },
    ]));
    expect(mocks.listGroups).toHaveBeenCalledTimes(2);
  });
});
