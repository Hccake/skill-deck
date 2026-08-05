/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen, within } from '@testing-library/react';
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
        { id: 'codex', displayName: 'Codex', detection: 'detected' },
        { id: 'warp', displayName: 'Warp', detection: 'notDetected' },
        { id: 'claude-code', displayName: 'Claude Code', detection: 'detected' },
        { id: 'cursor', displayName: 'Cursor', detection: 'notDetected' },
      ],
      directAgentIds: ['codex', 'warp'],
      items: [
        { id: 'claude', agentIds: ['claude-code'], category: 'separateInstall', displayName: 'Claude Code', path: '~/.claude/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null },
        { id: 'cursor', agentIds: ['cursor'], category: 'separateInstall', displayName: 'Cursor', path: '~/.cursor/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null },
      ],
      initialSelectedItemIds: ['claude'],
      unavailableExplicitAgents: [{ agentId: 'old-agent', reason: 'definitionMissing' }],
      requestedModeItemIds: ['claude', 'cursor'],
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
    context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
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
    selectedAgentItemIds: ['claude'],
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
  return { status: 'ready', inputKey: 'host/global', snapshot, retry: vi.fn() };
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

  it('keeps installation mode above the only scrolling list and publishes item IDs', async () => {
    const user = userEvent.setup();
    const { updateState } = renderStep();

    const title = screen.getByText('agentSelection.installTitle');
    const header = title.closest('header');
    expect(header?.contains(screen.getByRole('radiogroup', { name: 'agentSelection.modeTitle' }))).toBe(true);
    expect(screen.getByRole('radio', { name: 'agentSelection.linkRecommended' })).toBeDefined();
    await user.click(screen.getByRole('checkbox', { name: 'Claude Code' }));
    expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      selectedAgentItemIds: [],
      selectionRequiresReconfirmation: false,
    }));
  });

  it('allows choosing an installation mode before selecting an applicable Agent', async () => {
    const user = userEvent.setup();
    const { updateState } = renderStep(createState({ selectedAgentItemIds: [] }));

    const copyMode = screen.getByRole('radio', { name: 'agentSelection.copy' });
    expect(copyMode.hasAttribute('disabled')).toBe(false);
    await user.click(copyMode);

    expect(updateState).toHaveBeenCalledWith({ mode: 'copy' });
  });

  it('keeps the Agent heading when no item uses a selectable installation mode', () => {
    const snapshot = selectionSnapshot();
    snapshot.selection.requestedModeItemIds = [];
    renderStep(createState({ agentSelectionSnapshot: snapshot }), controller(snapshot));

    expect(screen.getByText('agentSelection.installTitle')).toBeDefined();
    expect(screen.queryByRole('radiogroup')).toBeNull();
  });

  it('shows the install empty state when no Agent is available', () => {
    const snapshot = selectionSnapshot();
    snapshot.selection.agents = [];
    snapshot.selection.directAgentIds = [];
    snapshot.selection.items = [];
    snapshot.selection.groups = [];
    snapshot.selection.initialSelectedItemIds = [];
    snapshot.selection.requestedModeItemIds = [];
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

  it('shows Eve placements as copy-only choices', () => {
    const snapshot = selectionSnapshot();
    snapshot.selection.agents.push({ id: 'eve', displayName: 'Eve', detection: 'detected' });
    snapshot.selection.items.push({
      id: 'eve-root',
      agentIds: ['eve'],
      category: 'groupChild',
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
      itemIds: ['eve-root'],
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
    snapshot.selection.items[0].selectable = true;
    snapshot.selection.items[0].disabledReason = 'placementConflict';
    snapshot.selection.initialSelectedItemIds = ['claude'];
    const { updateState } = renderStep(
      createState({ agentSelectionSnapshot: snapshot, selectedAgentItemIds: ['claude'] }),
      controller(snapshot),
    );

    expect(screen.getByText('agentSelection.disabled.placementConflict')).toBeDefined();
    await user.click(screen.getByRole('checkbox', { name: 'Claude Code' }));
    expect(updateState).toHaveBeenCalledWith(expect.objectContaining({ selectedAgentItemIds: [] }));
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
