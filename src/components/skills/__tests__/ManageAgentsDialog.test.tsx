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

  it('uses a compact, wider header with the Skill name and installation mode', () => {
    renderDialog();

    const title = screen.getByText('skills.manageAgents.title:{"name":"frontend-design"}');
    const header = title.closest('[data-slot="dialog-header"]');
    const description = screen.getByText(/^skills\.manageAgents\.description:/);
    const mode = screen.getByRole('radiogroup', { name: 'agentSelection.modeTitle' });
    const dialog = screen.getByRole('dialog', { name: 'skills.manageAgents.title:{"name":"frontend-design"}' });
    expect(header?.contains(mode)).toBe(true);
    expect(description.parentElement).toBe(header);
    expect(description.className).toContain('sr-only');
    expect(dialog.className).toContain('sm:max-w-3xl');
  });

  it('allows choosing the mode first without treating an inactive choice as a saved change', async () => {
    const user = userEvent.setup();
    renderDialog();

    const copyMode = screen.getByRole('radio', { name: 'agentSelection.copy' });
    expect(copyMode.hasAttribute('disabled')).toBe(false);
    await user.click(copyMode);

    expect(screen.getByRole('button', { name: 'skills.manageAgents.save' }).hasAttribute('disabled')).toBe(true);
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

  it('shows direct-use Agents as badges and reveals uncertain readers on hover', async () => {
    const user = userEvent.setup();
    const current = snapshot();
    current.selection.agents.push({ id: 'aider', displayName: 'Aider', detection: 'indeterminate' });
    current.selection.directAgentIds.push('aider');
    renderDialog({ snapshot: current });

    const codexBadge = screen.getByText('Codex').closest('[data-slot="direct-agent-badge"]');
    expect(codexBadge).not.toBeNull();
    expect(codexBadge?.querySelector('svg')).not.toBeNull();

    const more = screen.getByRole('button', { name: /^agentSelection\.moreAgents:/ });
    expect(more.closest('[data-slot="direct-agent-badge"]')).not.toBeNull();
    expect(more.querySelector('svg')).toBeNull();
    await user.hover(more);

    const popover = await screen.findByRole('dialog', { name: /^agentSelection\.moreAgents:/ });
    const description = within(popover).getByText('agentSelection.moreAgentsDescription');
    expect(description.className).toContain('whitespace-nowrap');
    for (const name of ['Warp', 'Aider']) {
      const badge = within(popover).getByText(name).closest('[data-slot="direct-agent-badge"]');
      expect(badge).not.toBeNull();
      expect(badge?.querySelector('svg')).not.toBeNull();
    }
  });

  it('keeps current installation states visible alongside each Agent', () => {
    const current = snapshot();
    current.selection.initialSelectedItemIds = ['claude', 'cursor'];
    current.itemStates[1] = {
      ...current.itemStates[1],
      currentEntry: 'copy',
      initialSelected: true,
      selectedEffect: 'retain',
      unselectedEffect: 'remove',
    };
    renderDialog({ snapshot: current });

    expect(screen.getByText('agentSelection.current.link')).toBeDefined();
    expect(screen.getByText('agentSelection.current.copy')).toBeDefined();
    const claudeRow = screen.getByRole('checkbox', { name: 'Claude Code' }).closest('[data-slot="agent-selection-row"]');
    const detectedStatus = within(claudeRow as HTMLElement).getByText('agentSelection.detection.detected');
    expect(detectedStatus.querySelector('[data-slot="agent-detection-dot"]')).not.toBeNull();
    expect(detectedStatus.querySelector('svg')).toBeNull();
  });

  it('connects a current installation state to its pending action', async () => {
    const user = userEvent.setup();
    renderDialog();

    const checkbox = screen.getByRole('checkbox', { name: 'Claude Code' });
    const row = checkbox.closest('[data-slot="agent-selection-row"]');
    await user.click(checkbox);

    expect(row?.textContent).toContain('agentSelection.current.link');
    expect(row?.textContent).toContain('→');
    expect(row?.textContent).toContain('agentSelection.effect.remove');
  });

  it('renders Other Agents as one disclosure control', () => {
    renderDialog();

    const trigger = screen.getByRole('button', { name: /agentSelection.otherAgents/ });
    expect(trigger.textContent).toContain('agentSelection.otherAgents');
  });

  it('keeps the Eve disclosure with its name and shows detection only on the parent', async () => {
    const user = userEvent.setup();
    renderDialog();

    const toggle = screen.getByRole('button', { name: /agentSelection.toggleGroup/ });
    const eveName = screen.getByText('Eve');
    expect(eveName.closest('label')?.parentElement).toBe(toggle.parentElement);
    await user.click(toggle);
    const group = screen.getByRole('group', { name: 'Eve' });
    const detection = within(group).getAllByText('agentSelection.detection.detected');
    expect(detection).toHaveLength(1);
    expect(within(group).getByText('agentSelection.copyOnly')).toBeDefined();
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
    const mergedCheckbox = screen.getByRole('checkbox', { name: /Cursor.*Windsurf/ });
    const mergedRow = mergedCheckbox.closest('[data-slot="agent-selection-row"]');
    expect(mergedRow?.querySelector('[data-slot="agent-group-glyph"]')).not.toBeNull();
    const detectedCount = within(mergedRow as HTMLElement).getByText(/^agentSelection\.detectedCount:/);
    expect(detectedCount.querySelector('[data-slot="agent-detection-dot"]')).not.toBeNull();
    expect(detectedCount.querySelector('svg')).toBeNull();
    expect(viewMembers.textContent).toContain('agentSelection.memberCount');
    act(() => viewMembers.focus());
    await user.keyboard('{Enter}');
    const membersPopover = await screen.findByRole('dialog', { name: 'agentSelection.viewMembers' });
    expect(within(membersPopover).getByText('agentSelection.sharedPlacementDescription')).toBeDefined();
    expect(within(membersPopover).getByText('Windsurf')).toBeDefined();
    const notDetectedStatus = within(membersPopover).getByText('agentSelection.detection.notDetected');
    expect(notDetectedStatus.querySelector('[data-slot="agent-detection-dot"]')).not.toBeNull();
    expect(notDetectedStatus.querySelector('svg')).toBeNull();
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
