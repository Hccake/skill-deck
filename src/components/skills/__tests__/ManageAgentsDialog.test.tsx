/* @vitest-environment jsdom */

import '@/test-utils';
import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  InstalledSkill,
  ManageAgentSelectionSnapshot,
} from '@/bindings';
import { TooltipProvider } from '@/components/ui/tooltip';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { useMutationStore } from '@/stores/mutation';
import { ManageAgentsDialog } from '../ManageAgentsDialog';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

vi.mock('@/components/recovery/RecoveryActions', () => ({
  RecoveryActions: ({ recovery }: { recovery: { resourceId: string } }) => (
    <span>recovery:{recovery.resourceId}</span>
  ),
}));

const skill: InstalledSkill = {
  name: 'frontend-design',
  description: 'Design skill',
  path: '/skills/frontend-design',
  canonicalPath: '/canonical/frontend-design',
  scope: 'global',
  agents: ['claude-code'],
  associatedAgents: ['claude-code'],
};

function snapshot(): ManageAgentSelectionSnapshot {
  return {
    selection: makeAgentSelectionSnapshot({
      agents: [
        { id: 'codex', displayName: 'Codex', detection: 'detected' },
        { id: 'warp', displayName: 'Warp', detection: 'notDetected' },
        { id: 'claude-code', displayName: 'Claude Code', detection: 'detected' },
        { id: 'cursor', displayName: 'Cursor', detection: 'detected' },
        { id: 'unknown-runtime', displayName: 'Unknown Runtime', detection: 'indeterminate' },
        { id: 'eve', displayName: 'Eve', detection: 'detected' },
      ],
      directAgentIds: ['codex', 'warp'],
      items: [
        { id: 'claude', agentIds: ['claude-code'], category: 'separateInstall', displayName: 'Claude Code', path: '~/.claude/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null },
        { id: 'cursor', agentIds: ['cursor'], category: 'separateInstall', displayName: 'Cursor', path: '~/.cursor/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null },
        { id: 'unknown', agentIds: ['unknown-runtime'], category: 'separateInstall', displayName: 'Unknown Runtime', path: '~/.unknown/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null },
        { id: 'eve-root', agentIds: ['eve'], category: 'groupChild', displayName: '主目录', path: '~/.eve/skills', groupId: 'eve-group', selectable: true, modeConstraint: 'copyOnly', disabledReason: null },
      ],
      groups: [{ id: 'eve-group', agentId: 'eve', displayName: 'Eve', itemIds: ['eve-root'], detection: 'detected' }],
      initialSelectedItemIds: ['claude'],
      unavailableExplicitAgents: [{ agentId: 'removed-agent', reason: 'definitionMissing' }],
      requestedModeItemIds: ['cursor', 'unknown'],
    }),
    itemStates: [
      { itemId: 'claude', currentEntry: 'link', initialSelected: true, allowedResults: 'both', selectedEffect: 'retain', unselectedEffect: 'remove', disabledReason: null },
      { itemId: 'cursor', currentEntry: 'none', initialSelected: false, allowedResults: 'both', selectedEffect: 'add', unselectedEffect: 'keepAbsent', disabledReason: null },
      { itemId: 'unknown', currentEntry: 'unrecognized', initialSelected: false, allowedResults: 'none', selectedEffect: null, unselectedEffect: null, disabledReason: 'unrecognizedEntry' },
      { itemId: 'eve-root', currentEntry: 'none', initialSelected: false, allowedResults: 'both', selectedEffect: 'add', unselectedEffect: 'keepAbsent', disabledReason: null },
    ],
  };
}

function renderDialog(props: Partial<React.ComponentProps<typeof ManageAgentsDialog>> = {}) {
  const onClose = vi.fn();
  const onSave = vi.fn().mockResolvedValue({ status: 'succeeded', response: { units: [] } });
  render(
    <TooltipProvider>
      <ManageAgentsDialog
        skill={skill}
        snapshot={snapshot()}
        onClose={onClose}
        onSave={onSave}
        {...props}
      />
    </TooltipProvider>,
  );
  return { onClose, onSave };
}

describe('ManageAgentsDialog', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('uses a fixed header and footer with the Agent list as the only scroll area', () => {
    renderDialog();

    const dialog = screen.getByRole('dialog');
    expect(dialog.className).toContain('grid-rows-[auto_minmax(0,1fr)_auto]');
    expect(screen.getByTestId('manage-agents-dialog-body').className).toContain('overflow-y-auto');
    expect(screen.getByRole('radiogroup', { name: 'agentSelection.modeTitle' })).toBeDefined();
  });

  it('renders loading and retry states without mounting a stale selection', async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();
    const { rerender } = render(
      <TooltipProvider>
        <ManageAgentsDialog skill={skill} loading onClose={vi.fn()} onSave={vi.fn()} />
      </TooltipProvider>,
    );
    expect(screen.getByRole('status')).toBeDefined();

    rerender(
      <TooltipProvider>
        <ManageAgentsDialog skill={skill} loadFailed onRetry={onRetry} onClose={vi.fn()} onSave={vi.fn()} />
      </TooltipProvider>,
    );
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.retryPreview' }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it('shows the manage empty state with only a close action', () => {
    const empty = snapshot();
    empty.selection.agents = [];
    empty.selection.directAgentIds = [];
    empty.selection.items = [];
    empty.selection.groups = [];
    empty.selection.initialSelectedItemIds = [];
    empty.selection.requestedModeItemIds = [];
    empty.itemStates = [];
    renderDialog({ snapshot: empty });

    expect(screen.getByText('agentSelection.manageEmpty')).toBeDefined();
    expect(screen.queryByRole('button', { name: 'common.cancel' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'skills.manageAgents.save' })).toBeNull();
  });

  it('shows direct-use Agents compactly and keeps uncertain readers in a tooltip', async () => {
    const user = userEvent.setup();
    renderDialog();

    expect(screen.getByText('Codex')).toBeDefined();
    const more = screen.getByText(/^agentSelection.moreAgents:/);
    await user.hover(more);
    const tooltip = await screen.findByRole('tooltip');
    expect(within(tooltip).getByText('agentSelection.moreAgentsDescription')).toBeDefined();
    expect(within(tooltip).getByText('Warp')).toBeDefined();
  });

  it('automatically reveals an undetected Agent whose directory entry is abnormal', () => {
    renderDialog();

    expect(screen.getByText('Unknown Runtime')).toBeDefined();
    expect(screen.queryByRole('checkbox', { name: 'Unknown Runtime' })).toBeNull();
    expect(screen.getByText('agentSelection.current.unrecognized')).toBeDefined();
  });

  it('submits opaque item IDs and the selected installation mode', async () => {
    const user = userEvent.setup();
    const { onSave } = renderDialog();

    await user.click(screen.getByRole('checkbox', { name: 'Cursor' }));
    await user.click(screen.getByRole('radio', { name: 'agentSelection.copy' }));
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));

    expect(onSave).toHaveBeenCalledWith({
      revision: 'selection-revision-1',
      selectedItemIds: ['claude', 'cursor'],
      requestedMode: 'copy',
    }, false);
  });

  it('requires an explicit second save when entity directories will be removed', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn()
      .mockResolvedValueOnce({ status: 'confirmationRequired' })
      .mockResolvedValueOnce({ status: 'succeeded', response: { units: [] } });
    renderDialog({ onSave });

    await user.click(screen.getByRole('checkbox', { name: 'Claude Code' }));
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.confirmRemoval' }));

    expect(onSave).toHaveBeenNthCalledWith(1, expect.any(Object), false);
    expect(onSave).toHaveBeenNthCalledWith(2, expect.any(Object), true);
  });

  it('discards from Cancel but confirms unsaved changes for the window close action', async () => {
    const user = userEvent.setup();
    const { onClose } = renderDialog();

    await user.click(screen.getByRole('checkbox', { name: 'Cursor' }));
    await user.click(screen.getByRole('button', { name: 'common.close' }));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('alert').textContent).toContain('skills.manageAgents.discardConfirm');

    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.continueEditing' }));
    await user.click(screen.getByRole('button', { name: 'common.cancel' }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('shows copy-only additions and exposes every member of a merged placement', async () => {
    const user = userEvent.setup();
    const merged = snapshot();
    merged.selection.agents.push({ id: 'windsurf', displayName: 'Windsurf', detection: 'notDetected' });
    merged.selection.items[1].agentIds.push('windsurf');
    renderDialog({ snapshot: merged });

    const viewMembers = screen.getByRole('button', { name: 'agentSelection.viewMembers' });
    act(() => viewMembers.focus());
    await user.keyboard('{Enter}');
    const membersPopover = await screen.findByRole('dialog', { name: 'agentSelection.viewMembers' });
    expect(within(membersPopover).getByText('Windsurf')).toBeDefined();
    await user.click(within(membersPopover).getByRole('button', { name: 'common.close' }));
    expect(document.activeElement).toBe(viewMembers);
    await user.click(screen.getByRole('button', { name: /agentSelection.toggleGroup/ }));
    await user.click(screen.getByRole('checkbox', { name: '主目录' }));
    expect(screen.getByText('agentSelection.effect.copy')).toBeDefined();
  });

  it('keeps the current choice and asks for confirmation when the selection snapshot changes', async () => {
    const user = userEvent.setup();
    const latest = snapshot();
    latest.selection.revision = 'selection-revision-2';
    latest.selection.initialSelectedItemIds = [];
    const onSave = vi.fn().mockResolvedValue({ status: 'stale' });
    const { rerender } = render(
      <TooltipProvider>
        <ManageAgentsDialog skill={skill} snapshot={snapshot()} onClose={vi.fn()} onSave={onSave} />
      </TooltipProvider>,
    );

    await user.click(screen.getByRole('checkbox', { name: 'Cursor' }));
    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.save' }));
    rerender(
      <TooltipProvider>
        <ManageAgentsDialog skill={skill} snapshot={latest} onClose={vi.fn()} onSave={onSave} />
      </TooltipProvider>,
    );

    expect(screen.getByRole('checkbox', { name: 'Cursor' }).getAttribute('data-state')).toBe('checked');
    expect(screen.getByRole('alert').textContent).toContain('agentSelection.selectionChanged');
    await user.click(screen.getByRole('button', { name: 'agentSelection.confirmCurrentSelection' }));
    expect(screen.queryByText('agentSelection.selectionChanged')).toBeNull();
  });
});
