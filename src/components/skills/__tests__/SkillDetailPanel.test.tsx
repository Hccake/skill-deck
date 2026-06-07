/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { SkillDetailPanel } from '../SkillDetailPanel';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { InstalledSkill } from '@/bindings';

const eventMocks = vi.hoisted(() => ({
  callback: null as null | ((event: { payload: { skillName: string; scope?: string; projectPath?: string | null; phase: string } }) => void),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en' },
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((_: string, callback: typeof eventMocks.callback) => {
    eventMocks.callback = callback;
    return Promise.resolve(() => {
      eventMocks.callback = null;
    });
  }),
}));

const makeSkill = (overrides: Partial<InstalledSkill> = {}): InstalledSkill => ({
  name: 'brainstorming',
  description: 'Brainstorm ideas',
  path: '/skills/brainstorming',
  canonicalPath: '/skills/cache/brainstorming',
  scope: 'global',
  agents: [],
  hasUpdate: true,
  canCheckForUpdates: true,
  ...overrides,
});

describe('SkillDetailPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.callback = null;
  });

  it('shows update progress instead of the update button while a skill is updating', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill()}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
        onClose={vi.fn()}
        onUpdate={vi.fn()}
        onDelete={vi.fn()}
        onRetry={vi.fn()}
        onManageAgents={vi.fn()}
        updateStatus="updating"
      />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
    expect(screen.getByText('skills.updatePhaseCloning')).toBeTruthy();
  });

  it('renders a check-updates action and triggers it', () => {
    const onCheckUpdates = vi.fn();

    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({ hasUpdate: false })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onCheckUpdates={onCheckUpdates as never}
        />
      </TooltipProvider>
    );

    fireEvent.click(screen.getByTitle('skills.checkUpdates'));

    expect(onCheckUpdates).toHaveBeenCalledTimes(1);
  });

  it('shows cannot-check status and reason without exposing update action when no update is available', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' }}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateStatus.cannotCheck')).toBeTruthy();
    expect(screen.getByText('skills.updateReason.missing-skill-path')).toBeTruthy();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('shows repair source action for missing skill path metadata', () => {
    const onRepairSource = vi.fn();

    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: false,
              canCheckForUpdates: false,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' }}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onRepairSource={onRepairSource}
        />
      </TooltipProvider>
    );

    fireEvent.click(screen.getByTitle('skills.actions.repairSource'));

    expect(onRepairSource).toHaveBeenCalledWith(expect.objectContaining({ name: 'brainstorming' }));
  });

  it('uses direct reinstall for missing version metadata', () => {
    const onUpdate = vi.fn();
    const onRepairSource = vi.fn();

    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: false,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              updateReason: 'missing-remote-hash',
            }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' }}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={onUpdate}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onRepairSource={onRepairSource}
        />
      </TooltipProvider>
    );

    fireEvent.click(screen.getByTitle('skills.actions.reinstall'));

    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByText('skills.reinstallConfirm.title')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'skills.reinstallConfirm.confirm' }));

    expect(onUpdate).toHaveBeenCalledWith('brainstorming', 'global');
    expect(onRepairSource).not.toHaveBeenCalled();
  });

  it('shows upstream-deleted state without ordinary update action', () => {
    const onUpdate = vi.fn();
    const onRepairSource = vi.fn();

    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: true,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              updateReason: 'deleted-upstream',
            }),
            updateStatus: 'deleted-upstream',
          } as InstalledSkill & { updateStatus?: 'deleted-upstream' }}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={onUpdate}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onRepairSource={onRepairSource}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateStatus.deleted-upstream')).toBeTruthy();
    expect(screen.getByText('skills.updateReason.deleted-upstream')).toBeTruthy();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();

    fireEvent.click(screen.getByTitle('skills.updatePlan.deletedUpstreamActionRepair'));

    expect(onRepairSource).toHaveBeenCalledWith(expect.objectContaining({ name: 'brainstorming' }));
    expect(onUpdate).not.toHaveBeenCalled();
  });

  it('hides ordinary update action when update cannot run even if stale update state is present', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({
            hasUpdate: true,
            canRunUpdate: false,
            updateReason: 'missing-skill-path',
          })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('hides update action for manual-only sources when no update is available', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({
            hasUpdate: false,
            canRunUpdate: true,
            canCheckForUpdates: false,
            updateReason: 'unsupported-source-type',
          })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it.each([
    ['rate-limited', 'skills.updateReason.rate-limited'],
    ['auth', 'skills.updateReason.auth'],
    ['network-error', 'skills.updateReason.network-error'],
    ['http-404', 'skills.updateReason.http-error'],
  ])('shows GitHub update reason %s', (reason, expectedKey) => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              updateReason: reason,
            }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' }}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText(expectedKey)).toBeTruthy();
  });

  it('hides the check-updates action when update-check capability metadata is missing', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({
            hasUpdate: false,
            canRunUpdate: false,
            canCheckForUpdates: undefined,
          })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onCheckUpdates={vi.fn(async () => true) as never}
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.checkUpdates')).toBeNull();
  });

  it('renders the description outside the title row so it keeps full width', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill()}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    const heading = screen.getByRole('heading', { name: 'brainstorming' });
    const description = screen.getByText('Brainstorm ideas');

    expect(heading.parentElement).not.toBe(description.parentElement);
  });

  it('shows duplicate copy maintenance prompt only when duplicate copy count is positive', () => {
    const { rerender } = render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({ duplicateCopyCount: 2 })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.detail.duplicateCopiesTitle')).toBeTruthy();

    rerender(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({ duplicateCopyCount: 0 })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('skills.detail.duplicateCopiesTitle')).toBeNull();
  });

  it('ignores update-progress events from a different skill identity', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({ scope: 'global' })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          updateStatus="updating"
        />
      </TooltipProvider>
    );

    act(() => {
      eventMocks.callback?.({
        payload: {
          skillName: 'brainstorming',
          scope: 'project',
          projectPath: 'D:\\Code\\other-project',
          phase: 'writing_lock',
        },
      });
    });

    expect(screen.queryByText('skills.updatePhaseWritingLock')).toBeNull();
    expect(screen.getByText('skills.updatePhaseCloning')).toBeTruthy();
  });

  it('resets the transient check-complete state when switching to a different skill', async () => {
    const { rerender } = render(
      <TooltipProvider>
        <SkillDetailPanel
          key="global:brainstorming"
          skill={makeSkill({ name: 'brainstorming', hasUpdate: false })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onCheckUpdates={vi.fn(async () => true) as never}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          isCheckingUpdates={false}
        />
      </TooltipProvider>
    );

    fireEvent.click(screen.getByTitle('skills.checkUpdates'));

    await waitFor(() => {
      expect(screen.getByTitle('skills.updateDone')).toBeTruthy();
    });

    rerender(
      <TooltipProvider>
        <SkillDetailPanel
          key="global:toolkit"
          skill={makeSkill({ name: 'toolkit', description: 'Toolkit', hasUpdate: false })}
          content="# Toolkit"
          loading={false}
          agentDisplayNames={new Map()}
          onCheckUpdates={vi.fn(async () => true) as never}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          isCheckingUpdates={false}
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.updateDone')).toBeNull();
  });

  it('resets the updating phase when switching to a different skill identity', () => {
    const { rerender } = render(
      <TooltipProvider>
        <SkillDetailPanel
          key="global:brainstorming"
          skill={makeSkill({ name: 'brainstorming', scope: 'global' })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          updateStatus="updating"
        />
      </TooltipProvider>
    );

    act(() => {
      eventMocks.callback?.({
        payload: {
          skillName: 'brainstorming',
          scope: 'global',
          phase: 'writing_lock',
        },
      });
    });

    expect(screen.getByText('skills.updatePhaseWritingLock')).toBeTruthy();

    rerender(
      <TooltipProvider>
        <SkillDetailPanel
          key="global:toolkit"
          skill={makeSkill({ name: 'toolkit', description: 'Toolkit', scope: 'global' })}
          content="# Toolkit"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          updateStatus="updating"
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('skills.updatePhaseWritingLock')).toBeNull();
    expect(screen.getByText('skills.updatePhaseCloning')).toBeTruthy();
  });
});
