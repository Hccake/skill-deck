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

  it('shows cannot-check status and reason while keeping update action when canRunUpdate is true', () => {
    const onUpdate = vi.fn();

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
          onUpdate={onUpdate}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateStatus.cannotCheck')).toBeTruthy();
    expect(screen.getByText('skills.updateReason.missing-skill-path')).toBeTruthy();
    fireEvent.click(screen.getByTitle('skills.actions.update'));
    expect(onUpdate).toHaveBeenCalledWith('brainstorming', 'global');
  });

  it('shows update action for manual-only sources before any update check runs', () => {
    const onUpdate = vi.fn();

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
          onUpdate={onUpdate}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    fireEvent.click(screen.getByTitle('skills.actions.update'));
    expect(onUpdate).toHaveBeenCalledWith('brainstorming', 'global');
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
