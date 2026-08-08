/* @vitest-environment jsdom */

import '@/test-utils';
import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SkillLocationRef, InstallAgentSelectionSnapshot } from '@/bindings';
import type { WizardState } from '@/components/skills/add-skill/types';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { useInstallTargetOptions } from '../useInstallTargetOptions';

const mocks = vi.hoisted(() => ({ getSelection: vi.fn() }));

vi.mock('@/hooks/useTauriApi', () => ({
  getInstallAgentSelection: (context: unknown, agents: unknown) => mocks.getSelection(context, agents),
}));

const context: SkillLocationRef = {
  environment: { kind: 'native' },
  scope: { scope: 'global' },
};

function snapshot(revision: string, selectedOptionIds = ['claude']): InstallAgentSelectionSnapshot {
  return {
    selection: makeAgentSelectionSnapshot({
      revision,
      agents: [{ kind: 'standard', id: 'claude-code', displayName: 'Claude Code', detection: 'detected', directoryAccess: 'privateOnly', installOptionId: 'claude', groupId: null }],
      installOptions: [{ id: 'claude', kind: 'standardDirectory', agentIds: ['claude-code'], displayName: 'Claude Code', path: '~/.claude/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null }],
      initialSelectedOptionIds: selectedOptionIds,
      userModeOptionIds: ['claude'],
    }),
    defaultSelectionWarning: null,
  };
}

function input(updateState: (updates: Partial<WizardState>) => void) {
  return {
    active: true,
    context,
    preselectedAgents: [] as string[],
    snapshot: null,
    selectedOptionIds: [] as string[],
    mode: 'symlink' as const,
    updateState,
  };
}

describe('useInstallTargetOptions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getSelection.mockResolvedValue(snapshot('revision-1'));
  });

  it('loads one coherent Backend snapshot and initializes the selection session', async () => {
    const updateState = vi.fn();
    const { result } = renderHook(() => useInstallTargetOptions(input(updateState)));

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(mocks.getSelection).toHaveBeenCalledWith(context, []);
    expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      selectedAgentOptionIds: ['claude'],
      selectionRequiresReconfirmation: false,
    }));
  });

  it('reuses the loaded snapshot when the Options step becomes active again', async () => {
    const updateState = vi.fn();
    const { result, rerender } = renderHook(
      ({ active }) => useInstallTargetOptions({ ...input(updateState), active }),
      { initialProps: { active: true } },
    );
    await waitFor(() => expect(result.current.status).toBe('ready'));

    rerender({ active: false });
    rerender({ active: true });
    await waitFor(() => expect(result.current.status).toBe('ready'));

    expect(mocks.getSelection).toHaveBeenCalledOnce();
  });

  it('does not publish a late response from a previous Context', async () => {
    let resolveHost!: (value: InstallAgentSelectionSnapshot) => void;
    mocks.getSelection
      .mockReturnValueOnce(new Promise((resolve) => { resolveHost = resolve; }))
      .mockResolvedValueOnce(snapshot('revision-wsl'));
    const updateState = vi.fn();
    const wslContext: SkillLocationRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    const { result, rerender } = renderHook(
      ({ currentContext }) => useInstallTargetOptions({ ...input(updateState), context: currentContext }),
      { initialProps: { currentContext: context } },
    );

    rerender({ currentContext: wslContext });
    await waitFor(() => expect(result.current.status).toBe('ready'));
    await act(async () => resolveHost(snapshot('revision-native')));

    expect(updateState).toHaveBeenCalledTimes(1);
    expect(result.current.status === 'ready' && result.current.snapshot.selection.revision).toBe('revision-wsl');
  });

  it('retries a failed snapshot request', async () => {
    mocks.getSelection.mockRejectedValueOnce(new Error('offline'));
    const { result } = renderHook(() => useInstallTargetOptions(input(vi.fn())));
    await waitFor(() => expect(result.current.status).toBe('error'));

    await act(async () => result.current.retry());
    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(mocks.getSelection).toHaveBeenCalledTimes(2);
  });
});
