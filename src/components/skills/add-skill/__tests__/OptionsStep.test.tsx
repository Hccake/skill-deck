/* @vitest-environment jsdom */

import '@/test-utils';
import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { InstallAgentSelectionSnapshot } from '@/bindings';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  useAgentSelectionSession,
  type AgentSelectionSessionController,
} from '@/hooks/useAgentSelectionSession';
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
    selectionHistoryWarning: null,
  };
}

function ReadyOptions({
  snapshot,
  refreshedSnapshot,
}: {
  snapshot: InstallAgentSelectionSnapshot;
  refreshedSnapshot?: InstallAgentSelectionSnapshot;
}) {
  const request = {
    kind: 'install' as const,
    context: { environment: { kind: 'native' as const }, scope: { scope: 'global' as const } },
    explicitAgentIds: [],
  };
  const agentSelection = useAgentSelectionSession({
    active: true,
    request,
    load: async () => snapshot,
  });
  return (
    <>
      <OptionsStep agentSelection={agentSelection} />
      {refreshedSnapshot ? (
        <button
          type="button"
          onClick={() => agentSelection.acceptSnapshot(refreshedSnapshot)}
        >
          refresh-selection
        </button>
      ) : null}
    </>
  );
}

async function renderStep(
  snapshot = selectionSnapshot(),
  options: { refreshedSnapshot?: InstallAgentSelectionSnapshot } = {},
) {
  render(
    <TooltipProvider>
      <ReadyOptions snapshot={snapshot} {...options} />
    </TooltipProvider>,
  );
  await screen.findByText('agentSelection.installTitle');
}

function unreadyController(
  status: 'loading' | 'error',
  retry: () => Promise<void>,
): AgentSelectionSessionController<InstallAgentSelectionSnapshot> {
  const actions = {
    retry,
    setOptionSelected: vi.fn(),
    setMode: vi.fn(),
    setGroupSelected: vi.fn(),
    setOtherAgentsExpanded: vi.fn(),
    setAdditionalInstallExpanded: vi.fn(),
    setGroupExpanded: vi.fn(),
    acceptSnapshot: vi.fn(),
    confirmCurrentSelection: vi.fn(),
  };
  return status === 'error'
    ? { status, error: { kind: 'custom', data: { message: 'failed' } }, ...actions }
    : { status, ...actions };
}

describe('OptionsStep', () => {
  it('leaves readiness to the Agent selection controller', () => {
    expect(canProceedForStep({ step: 'options' } as WizardState)).toBe(true);
  });

  it('renders loading and retry states', async () => {
    const user = userEvent.setup();
    const retry = vi.fn(async () => undefined);
    const { rerender } = render(
      <OptionsStep agentSelection={unreadyController('loading', retry)} />,
    );
    expect(screen.getByRole('status')).toBeDefined();

    rerender(<OptionsStep agentSelection={unreadyController('error', retry)} />);
    await user.click(screen.getByRole('button', { name: 'common.retry' }));
    expect(retry).toHaveBeenCalledOnce();
  });

  it('keeps installation mode above the only scrolling list and updates selection', async () => {
    const user = userEvent.setup();
    await renderStep();

    const title = screen.getByText('agentSelection.installTitle');
    const header = title.closest('header');
    expect(header?.contains(screen.getByRole('radiogroup', { name: 'agentSelection.modeTitle' }))).toBe(true);
    const checkbox = screen.getByRole('checkbox', { name: 'Claude Code' });
    await user.click(checkbox);
    expect(checkbox.getAttribute('data-state')).toBe('unchecked');
  });

  it('uses install-specific guidance for automatic and selectable Agents', async () => {
    await renderStep();
    expect(screen.getByText('agentSelection.automatic.install.title')).toBeDefined();
    const selectableHelp = screen.getByRole('button', { name: 'agentSelection.selectable.help' });
    act(() => selectableHelp.focus());
    expect((await screen.findByRole('tooltip')).textContent).toContain('agentSelection.selectable.help');
  });

  it('allows choosing an installation mode before selecting an applicable Agent', async () => {
    const user = userEvent.setup();
    const snapshot = selectionSnapshot();
    snapshot.selection.initialSelectedOptionIds = [];
    await renderStep(snapshot);
    const copyMode = screen.getByRole('radio', { name: 'agentSelection.copy' });
    await user.click(copyMode);
    expect(copyMode.getAttribute('data-state')).toBe('checked');
  });

  it('keeps the Agent heading when no item uses a selectable installation mode', async () => {
    const snapshot = selectionSnapshot();
    snapshot.selection.userModeOptionIds = [];
    await renderStep(snapshot);
    expect(screen.getByText('agentSelection.installTitle')).toBeDefined();
    expect(screen.queryByRole('radiogroup')).toBeNull();
  });

  it('shows the install empty state when no Agent is available', async () => {
    const snapshot = selectionSnapshot();
    snapshot.selection.agents = [];
    snapshot.selection.installOptions = [];
    snapshot.selection.groups = [];
    snapshot.selection.initialSelectedOptionIds = [];
    snapshot.selection.userModeOptionIds = [];
    await renderStep(snapshot);
    expect(screen.getByText('agentSelection.installEmpty')).toBeDefined();
  });

  it('shows unknown explicit Agents before the selection header', async () => {
    await renderStep();
    const notice = screen.getByRole('status');
    const header = screen.getByText('agentSelection.installTitle').closest('header');
    expect(notice.textContent).toContain('old-agent');
    expect(notice.compareDocumentPosition(header as Node) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
  });

  it('keeps undetected separate Agents under the collapsed Other Agents section', async () => {
    const user = userEvent.setup();
    await renderStep();
    expect(screen.queryByText('Cursor')).toBeNull();
    await user.click(screen.getByText(/agentSelection.otherAgents/));
    expect(screen.getByText('Cursor')).toBeDefined();
  });

  it('keeps optional own-directory writes inside the direct-use section', async () => {
    const user = userEvent.setup();
    const snapshot = selectionSnapshot();
    snapshot.selection.agents.push({ kind: 'standard', id: 'zed', displayName: 'Zed', detection: 'detected', directoryAccess: 'both', installOptionId: 'zed', groupId: null });
    snapshot.selection.installOptions.push({ id: 'zed', kind: 'standardDirectory', agentIds: ['zed'], displayName: 'Zed', path: '~/.zed/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null });
    snapshot.selection.userModeOptionIds.push('zed');
    await renderStep(snapshot);
    const directSection = screen.getByText('agentSelection.automatic.install.title').closest('section');
    await user.click(within(directSection as HTMLElement).getByRole('button', { name: /agentSelection\.ownDirectory\.title/ }));
    expect(screen.getByRole('checkbox', { name: 'Zed' })).toBeDefined();
  });

  it('shows grouped placements as copy-only choices', async () => {
    const user = userEvent.setup();
    const snapshot = selectionSnapshot();
    snapshot.selection.agents.push({ kind: 'grouped', id: 'eve', displayName: 'Eve', detection: 'detected', directoryAccess: null, installOptionId: null, groupId: 'eve-group' });
    snapshot.selection.installOptions.push({ id: 'eve-root', kind: 'groupLocation', agentIds: ['eve'], displayName: 'Main', path: '~/.eve/skills', groupId: 'eve-group', selectable: true, modeConstraint: 'copyOnly', disabledReason: null });
    snapshot.selection.groups.push({ id: 'eve-group', agentId: 'eve', displayName: 'Eve', optionIds: ['eve-root'], detection: 'detected' });
    await renderStep(snapshot);
    await user.click(screen.getByRole('button', { name: /agentSelection.toggleGroup/ }));
    expect(within(screen.getByRole('group', { name: 'Eve' })).getByText('agentSelection.copyOnly')).toBeDefined();
  });

  it('allows an initially selected placement conflict to be canceled', async () => {
    const user = userEvent.setup();
    const snapshot = selectionSnapshot();
    snapshot.selection.installOptions[0].disabledReason = 'placementConflict';
    await renderStep(snapshot);
    const checkbox = screen.getByRole('checkbox', { name: 'Claude Code' });
    await user.click(checkbox);
    expect(checkbox.getAttribute('data-state')).toBe('unchecked');
  });

  it('shows a saved-default warning without blocking the selection', async () => {
    const snapshot = selectionSnapshot();
    snapshot.selectionHistoryWarning = 'readFailed';
    await renderStep(snapshot);
    expect(screen.getByText('addSkill.agents.historyLoadWarning')).toBeDefined();
    expect(screen.getByRole('checkbox', { name: 'Claude Code' })).toBeDefined();
  });

  it('requires the user to confirm a refreshed Agent selection before continuing', async () => {
    const user = userEvent.setup();
    const latest = selectionSnapshot();
    latest.selection.revision = 'selection-revision-2';
    await renderStep(selectionSnapshot(), { refreshedSnapshot: latest });
    await user.click(screen.getByRole('button', { name: 'refresh-selection' }));
    expect(screen.getByRole('alert').textContent).toContain('agentSelection.selectionChanged');
    await user.click(screen.getByRole('button', { name: 'agentSelection.confirmCurrentSelection' }));
    expect(screen.queryByText('agentSelection.selectionChanged')).toBeNull();
  });
});
