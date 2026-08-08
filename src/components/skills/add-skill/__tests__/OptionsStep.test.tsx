/* @vitest-environment jsdom */

import '@/test-utils';
import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { InstallAgentSelectionSnapshot } from '@/bindings';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { InstallTargetOptionsController } from '@/hooks/useInstallTargetOptions';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { canProceedForStep, type WizardState } from '../types';
import { OptionsStep } from '../OptionsStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

function selectionSnapshot(): InstallAgentSelectionSnapshot {
  return {
    selection: makeAgentSelectionSnapshot({
      agents: [
        { kind: 'standard', id: 'codex', displayName: 'Codex', detection: 'detected', directoryAccess: 'standardOnly', installOptionId: null, groupId: null },
        { kind: 'standard', id: 'warp', displayName: 'Warp', detection: 'notDetected', directoryAccess: 'standardOnly', installOptionId: null, groupId: null },
        { kind: 'standard', id: 'claude-code', displayName: 'Claude Code', detection: 'detected', directoryAccess: 'privateOnly', installOptionId: 'claude', groupId: null },
        { kind: 'standard', id: 'cursor', displayName: 'Cursor', detection: 'notDetected', directoryAccess: 'privateOnly', installOptionId: 'cursor', groupId: null },
      ],
      installOptions: [
        { id: 'claude', kind: 'standardDirectory', agentIds: ['claude-code'], displayName: 'Claude Code', path: '~/.claude/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null },
        { id: 'cursor', kind: 'standardDirectory', agentIds: ['cursor'], displayName: 'Cursor', path: '~/.cursor/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null },
      ],
      initialSelectedOptionIds: ['claude'],
      unavailableExplicitAgents: [{ agentId: 'old-agent', reason: 'definitionMissing' }],
      userModeOptionIds: ['claude', 'cursor'],
    }),
    defaultSelectionWarning: null,
  };
}

function createState(overrides: Partial<WizardState> = {}): WizardState {
  const snapshot = selectionSnapshot();
  return {
    step: 'options',
    entryPoint: 'skills-panel',
    scope: 'global',
    context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
    source: 'owner/repo',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    riskPolicy: null,
    riskAcknowledged: false,
    availableSkills: [],
    selectedSkills: ['demo'],
    skillFilter: null,
    skillSearchQuery: '',
    agentSelectionSnapshot: snapshot,
    selectedAgentOptionIds: ['claude'],
    expandedAgentGroupIds: [],
    additionalAgentsExpanded: false,
    selectionRequiresReconfirmation: false,
    mode: 'symlink',
    otherAgentsExpanded: false,
    overwrites: {},
    preparation: { status: 'idle' },
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    ...overrides,
  };
}

function controller(snapshot = selectionSnapshot()): InstallTargetOptionsController {
  return { status: 'ready', inputKey: 'native/global', snapshot, retry: vi.fn() };
}

function renderStep(state = createState(), targetOptions = controller()) {
  const updateState = vi.fn();
  render(
    <TooltipProvider>
      <OptionsStep state={state} updateState={updateState} targetOptions={targetOptions} />
    </TooltipProvider>,
  );
  return { updateState };
}

describe('OptionsStep', () => {
  it('only allows continuing after a current snapshot is confirmed', () => {
    expect(canProceedForStep(createState())).toBe(true);
    expect(canProceedForStep(createState({ selectionRequiresReconfirmation: true }))).toBe(false);
    expect(canProceedForStep(createState({ agentSelectionSnapshot: null }))).toBe(false);
  });

  it('renders loading and retry states', async () => {
    const user = userEvent.setup();
    const retry = vi.fn();
    const state = createState({ agentSelectionSnapshot: null });
    const { rerender } = render(
      <OptionsStep state={state} updateState={vi.fn()} targetOptions={{ status: 'loading', inputKey: 'key', retry }} />,
    );
    expect(screen.getByRole('status')).toBeDefined();

    rerender(
      <OptionsStep state={state} updateState={vi.fn()} targetOptions={{ status: 'error', inputKey: 'key', error: { kind: 'custom', data: { message: 'failed' } }, retry }} />,
    );
    await user.click(screen.getByRole('button', { name: 'common.retry' }));
    expect(retry).toHaveBeenCalledOnce();
  });

  it('keeps installation mode above the only scrolling list and publishes option IDs', async () => {
    const user = userEvent.setup();
    const { updateState } = renderStep();

    const title = screen.getByText('agentSelection.installTitle');
    const header = title.closest('header');
    expect(header?.contains(screen.getByRole('radiogroup', { name: 'agentSelection.modeTitle' }))).toBe(true);
    expect(screen.getByRole('radio', { name: 'agentSelection.linkRecommended' })).toBeDefined();
    await user.click(screen.getByRole('checkbox', { name: 'Claude Code' }));
    expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      selectedAgentOptionIds: [],
      selectionRequiresReconfirmation: false,
    }));
  });

  it('uses install-specific guidance for automatic and selectable Agents', async () => {
    renderStep();

    expect(screen.getByText('agentSelection.automatic.install.title')).toBeDefined();
    expect(screen.getByText('agentSelection.selectable.title')).toBeDefined();

    const selectableHelp = screen.getByRole('button', {
      name: 'agentSelection.selectable.help',
    });
    act(() => selectableHelp.focus());
    expect((await screen.findByRole('tooltip')).textContent).toContain(
      'agentSelection.selectable.help',
    );
  });

  it('allows choosing an installation mode before selecting an applicable Agent', async () => {
    const user = userEvent.setup();
    const { updateState } = renderStep(createState({ selectedAgentOptionIds: [] }));

    const copyMode = screen.getByRole('radio', { name: 'agentSelection.copy' });
    expect(copyMode.hasAttribute('disabled')).toBe(false);
    await user.click(copyMode);

    expect(updateState).toHaveBeenCalledWith({ mode: 'copy' });
  });

  it('keeps the Agent heading when no item uses a selectable installation mode', () => {
    const snapshot = selectionSnapshot();
    snapshot.selection.userModeOptionIds = [];
    renderStep(createState({ agentSelectionSnapshot: snapshot }), controller(snapshot));

    expect(screen.getByText('agentSelection.installTitle')).toBeDefined();
    expect(screen.queryByRole('radiogroup')).toBeNull();
  });

  it('shows the install empty state when no Agent is available', () => {
    const snapshot = selectionSnapshot();
    snapshot.selection.agents = [];
    snapshot.selection.installOptions = [];
    snapshot.selection.groups = [];
    snapshot.selection.initialSelectedOptionIds = [];
    snapshot.selection.userModeOptionIds = [];
    renderStep(createState({ agentSelectionSnapshot: snapshot }), controller(snapshot));

    expect(screen.getByText('agentSelection.installEmpty')).toBeDefined();
  });

  it('shows unknown explicit Agents as one read-only notice', () => {
    renderStep();

    const notice = screen.getByRole('status');
    const header = screen.getByText('agentSelection.installTitle').closest('header');
    expect(notice.textContent).toContain('old-agent');
    expect(header).not.toBeNull();
    expect(notice.compareDocumentPosition(header as Node) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(screen.queryByRole('checkbox', { name: 'old-agent' })).toBeNull();
  });

  it('keeps undetected separate Agents under the collapsed Other Agents section', async () => {
    const user = userEvent.setup();
    const { updateState } = renderStep();

    expect(screen.queryByText('Cursor')).toBeNull();
    await user.click(screen.getByText(/agentSelection.otherAgents/));
    expect(updateState).toHaveBeenCalledWith(expect.objectContaining({ otherAgentsExpanded: true }));
  });

  it('publishes the optional own-directory disclosure from the direct-use section', async () => {
    const user = userEvent.setup();
    const snapshot = selectionSnapshot();
    snapshot.selection.agents.push({ kind: 'standard', id: 'zed', displayName: 'Zed', detection: 'detected', directoryAccess: 'both', installOptionId: 'zed', groupId: null });
    snapshot.selection.installOptions.push({ id: 'zed', kind: 'standardDirectory', agentIds: ['zed'], displayName: 'Zed', path: '~/.zed/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null });
    snapshot.selection.userModeOptionIds.push('zed');
    const { updateState } = renderStep(
      createState({ agentSelectionSnapshot: snapshot }),
      controller(snapshot),
    );

    const directSection = screen.getByText('agentSelection.automatic.install.title').closest('section');
    expect(directSection).not.toBeNull();
    await user.click(within(directSection as HTMLElement).getByRole('button', {
      name: /agentSelection\.ownDirectory\.title/,
    }));

    expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      additionalAgentsExpanded: true,
    }));
  });

  it('shows Eve placements as copy-only choices', () => {
    const snapshot = selectionSnapshot();
    snapshot.selection.agents.push({ kind: 'grouped', id: 'eve', displayName: 'Eve', detection: 'detected', directoryAccess: null, installOptionId: null, groupId: 'eve-group' });
    snapshot.selection.installOptions.push({
      id: 'eve-root',
      kind: 'groupLocation',
      agentIds: ['eve'],
      displayName: '主目录',
      path: '~/.eve/skills',
      groupId: 'eve-group',
      selectable: true,
      modeConstraint: 'copyOnly',
      disabledReason: null,
    });
    snapshot.selection.groups.push({
      id: 'eve-group',
      agentId: 'eve',
      displayName: 'Eve',
      optionIds: ['eve-root'],
      detection: 'detected',
    });
    renderStep(createState({
      agentSelectionSnapshot: snapshot,
      expandedAgentGroupIds: ['eve-group'],
    }), controller(snapshot));

    const group = screen.getByRole('group', { name: 'Eve' });
    expect(within(group).getByText('agentSelection.copyOnly')).toBeDefined();
    expect(within(group).getByText('agentSelection.copy')).toBeDefined();
  });

  it('allows an initially selected placement conflict to be canceled', async () => {
    const user = userEvent.setup();
    const snapshot = selectionSnapshot();
    snapshot.selection.installOptions[0].selectable = true;
    snapshot.selection.installOptions[0].disabledReason = 'placementConflict';
    snapshot.selection.initialSelectedOptionIds = ['claude'];
    const { updateState } = renderStep(
      createState({ agentSelectionSnapshot: snapshot, selectedAgentOptionIds: ['claude'] }),
      controller(snapshot),
    );

    expect(screen.getByText('agentSelection.disabled.placementConflict')).toBeDefined();
    await user.click(screen.getByRole('checkbox', { name: 'Claude Code' }));
    expect(updateState).toHaveBeenCalledWith(expect.objectContaining({ selectedAgentOptionIds: [] }));
  });

  it('shows a saved-default warning without blocking the selection', () => {
    const snapshot = selectionSnapshot();
    snapshot.defaultSelectionWarning = 'readFailed';
    renderStep(createState({ agentSelectionSnapshot: snapshot }), controller(snapshot));

    expect(screen.getByText('addSkill.agents.defaultLoadWarning')).toBeDefined();
    expect(screen.getByRole('checkbox', { name: 'Claude Code' })).toBeDefined();
  });

  it('requires the user to confirm a refreshed Agent selection before continuing', async () => {
    const user = userEvent.setup();
    const { updateState } = renderStep(createState({ selectionRequiresReconfirmation: true }));

    expect(screen.getByRole('alert').textContent).toContain('agentSelection.selectionChanged');
    await user.click(screen.getByRole('button', { name: 'agentSelection.confirmCurrentSelection' }));

    expect(updateState).toHaveBeenCalledWith({ selectionRequiresReconfirmation: false });
  });
});
