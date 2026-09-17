/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { SkillCard } from '../SkillCard';
import type { InstalledSkill } from '@/bindings';

const eventMocks = vi.hoisted(() => ({
  callback: null as null | ((event: { payload: { skillName: string; scope?: string; projectPath?: string | null; phase: string } }) => void),
}));

const openerMocks = vi.hoisted(() => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
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

vi.mock('@tauri-apps/plugin-opener', () => openerMocks);

vi.mock('sonner', () => ({
  toast: toastMocks,
}));

const makeSkill = (overrides: Partial<InstalledSkill> = {}): InstalledSkill => ({
  name: 'toolkit',
  description: 'Toolkit',
  path: '/skills/toolkit',
  canonicalPath: '/canonical/toolkit',
  scope: 'global',
  agents: [],
  associatedAgents: [],
  hasUpdate: true,
  ...overrides,
});

describe('SkillCard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.callback = null;
  });

  it('keeps the Skill name and plugin name in one title row', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({ pluginName: 'claude-code-tools' })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    const titleRow = screen.getByTestId('skill-card-title');
    expect(within(titleRow).getByText('toolkit')).toBeTruthy();
    expect(within(titleRow).getByText('Claude Code Tools')).toBeTruthy();
  });

  it('keeps the primary status visible when the Skill and plugin names are long', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            name: 'a-very-long-skill-name-that-needs-to-truncate',
            pluginName: 'a-very-long-plugin-name-that-needs-to-truncate-first',
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    const titleRow = screen.getByTestId('skill-card-title');
    const status = within(titleRow).getByText('skills.updateStatusLabel.available');
    expect(status.parentElement?.parentElement?.className).toContain('shrink-0');
  });

  it('shows source, update, and duplicate-install facts in one attention row', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: false,
              canCheckForUpdates: false,
              updateReason: 'missing-skill-path',
              duplicateCopyCount: 2,
            }),
            updateStatus: 'cannotCheck',
            updateAttempt: { outcome: 'notCompleted', reason: 'upstreamUnavailable' },
          } as never}
          displayScope="global"
          hasDuplicateLocation
        />
      </TooltipProvider>
    );

    const attention = screen.getByTestId('skill-card-attention');
    expect(within(attention).getByText('skills.card.sourceIncomplete')).toBeTruthy();
    expect(within(attention).getByText('skills.card.duplicateLocations')).toBeTruthy();
    expect(within(attention).getByText('skills.card.duplicateAgentInstall')).toBeTruthy();
    expect(within(attention).queryByText('skills.card.updateCheckIncomplete')).toBeNull();
    expect(attention.querySelectorAll('svg')).toHaveLength(1);
    expect(screen.queryByText('skills.updateStatusLabel.needsSourceInfo')).toBeNull();
  });

  it('keeps duplicate-install facts without repeating an incomplete background check', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: true,
              canRunUpdate: true,
              canCheckForUpdates: true,
            }),
            updateStatus: 'updateAvailable',
            updateAttempt: { outcome: 'notCompleted', reason: 'upstreamUnavailable' },
          } as never}
          displayScope="project"
          hasDuplicateLocation
        />
      </TooltipProvider>
    );

    const attention = screen.getByTestId('skill-card-attention');
    expect(within(attention).queryByText('skills.card.updateCheckIncomplete')).toBeNull();
    expect(within(attention).getByText('skills.card.duplicateLocations')).toBeTruthy();
    expect(attention.querySelectorAll('svg')).toHaveLength(1);
  });

  it('keeps copy, Agent management, and delete available as direct actions', async () => {
    const onCopyToProject = vi.fn();
    const onManageAgents = vi.fn();
    const onDelete = vi.fn();
    const onClick = vi.fn();
    const user = userEvent.setup();

    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({ scope: 'project', hasUpdate: false })}
          displayScope="project"
          onCopyToProject={onCopyToProject}
          onManageAgents={onManageAgents}
          onDelete={onDelete}
          onClick={onClick}
        />
      </TooltipProvider>
    );

    await user.click(screen.getByRole('button', { name: 'skills.actions.copyToProject' }));
    expect(onCopyToProject).toHaveBeenCalledWith(expect.objectContaining({ name: 'toolkit' }));

    await user.click(screen.getByRole('button', { name: 'skills.manageAgents.action' }));
    expect(onManageAgents).toHaveBeenCalledWith(expect.objectContaining({ name: 'toolkit' }));

    await user.click(screen.getByRole('button', { name: 'skills.actions.delete' }));
    expect(onDelete).toHaveBeenCalledWith(expect.objectContaining({ name: 'toolkit' }));
    expect(onClick).not.toHaveBeenCalled();
  });

  it('opens an actionable source with the desktop opener without selecting the card', async () => {
    const onClick = vi.fn();
    const user = userEvent.setup();
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
          })}
          displayScope="global"
          onClick={onClick}
        />
      </TooltipProvider>
    );

    await user.click(screen.getByRole('button', { name: 'owner/repo' }));

    expect(openerMocks.openUrl).toHaveBeenCalledWith('https://github.com/owner/repo');
    expect(onClick).not.toHaveBeenCalled();
  });

  it('reports an external source open failure to the user', async () => {
    openerMocks.openUrl.mockRejectedValueOnce(new Error('desktop opener failed'));
    const user = userEvent.setup();
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    await user.click(screen.getByRole('button', { name: 'owner/repo' }));

    await waitFor(() => {
      expect(toastMocks.error).toHaveBeenCalledWith('skills.card.sourceOpenFailed');
    });
  });

  it('renders source as plain metadata when it has no openable URL', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({ source: 'local/toolkit', sourceUrl: null })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.getByText('local/toolkit')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'local/toolkit' })).toBeNull();
  });

  it('disables every card write action when writes are blocked', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({ scope: 'project', canRunUpdate: true })}
          displayScope="project"
          writeBlocked
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onCopyToProject={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    for (const name of [
      'skills.actions.update',
      'skills.actions.copyToProject',
      'skills.manageAgents.action',
      'skills.actions.delete',
    ]) {
      expect((screen.getByRole('button', { name }) as HTMLButtonElement).disabled).toBe(true);
    }
  });


  it.each([
    ['acquiring', 'skills.updatePhaseAcquiring'],
    ['validating', 'skills.updatePhaseValidating'],
    ['updating', 'skills.updatePhaseUpdating'],
  ] as const)('shows the %s workflow phase accurately', (updateStatus, label) => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({ scope: 'global' })}
          displayScope="global"
          updateStatus={updateStatus}
        />
      </TooltipProvider>
    );

    expect(screen.getByText(label)).toBeTruthy();
  });

  it('shows a missing source only in the attention row', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.card.sourceIncomplete')).toBeTruthy();
    expect(screen.queryByText('skills.updateStatusLabel.needsSourceInfo')).toBeNull();
    expect(screen.queryByText('skills.updateHint.missing-skill-path')).toBeNull();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('keeps the single skill update action as the primary action when an update is available', () => {
    const onUpdate = vi.fn();
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: true,
            canRunUpdate: true,
          })}
          displayScope="global"
          onUpdate={onUpdate}
        />
      </TooltipProvider>
    );

    expect(screen.getByTitle('skills.actions.update')).toBeTruthy();
    expect(screen.getByText('skills.updateStatusLabel.available')).toBeTruthy();

    fireEvent.click(screen.getByTitle('skills.actions.update'));
    expect(onUpdate).toHaveBeenCalledWith('toolkit');
  });

  it('renders concrete card agent names and excludes private-copy-only agents', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            associatedAgents: ['claude-code', 'codex'],
            defaultAvailableAgents: ['claude-code'],
            privateAdaptedAgents: ['codex'],
            privateCopyAgents: ['gemini-cli'],
          })}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
            ['gemini-cli', 'Gemini'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('Claude Code')).toBeTruthy();
    expect(screen.getByText('Codex')).toBeTruthy();
    expect(screen.queryByText('Gemini')).toBeNull();
  });

  it('does not render agent availability category count keys on the card', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            associatedAgents: ['claude-code', 'codex'],
            defaultAvailableAgents: ['claude-code'],
            privateAdaptedAgents: ['codex'],
            privateCopyAgents: ['gemini-cli'],
          })}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
            ['gemini-cli', 'Gemini'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('skills.detail.defaultAvailableCount')).toBeNull();
    expect(screen.queryByText('skills.detail.privateAdaptedCount')).toBeNull();
    expect(screen.queryByText('skills.detail.privateCopyCount')).toBeNull();
  });

  it('shows the Agent-directory installation fact only when duplicate entries are reported', () => {
    const { rerender } = render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            associatedAgents: ['claude-code'],
            privateCopyAgents: ['codex'],
            duplicateCopyCount: 0,
          })}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('Claude Code')).toBeTruthy();
    expect(screen.queryByText('skills.card.duplicateAgentInstall')).toBeNull();

    rerender(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            associatedAgents: ['claude-code'],
            privateCopyAgents: [],
            duplicateCopyCount: 2,
          })}
          displayScope="global"
          agentDisplayNames={new Map([['claude-code', 'Claude Code']])}
        />
      </TooltipProvider>
    );

    const attention = screen.getByTestId('skill-card-attention');
    expect(within(attention).getByText('skills.card.duplicateAgentInstall')).toBeTruthy();
    expect(attention.querySelectorAll('svg')).toHaveLength(1);
  });

  it('renders all card agent names without an overflow chip', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            associatedAgents: ['claude-code', 'codex', 'gemini-cli', 'cursor', 'qwen-code'],
          })}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
            ['gemini-cli', 'Gemini'],
            ['cursor', 'Cursor'],
            ['qwen-code', 'Qwen'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('Claude Code')).toBeTruthy();
    expect(screen.getByText('Codex')).toBeTruthy();
    expect(screen.getByText('Gemini')).toBeTruthy();
    expect(screen.getByText('Cursor')).toBeTruthy();
    expect(screen.getByText('Qwen')).toBeTruthy();
    expect(screen.queryByText('skills.card.moreAgents')).toBeNull();
  });

  it('renders the explicit associated Agent projection and ignores summary-only fields', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            associatedAgents: ['claude-code', 'codex', 'gemini-cli'],
            defaultAvailableAgents: ['claude-code', 'codex'],
            privateAdaptedAgents: ['codex', 'gemini-cli'],
            privateCopyAgents: ['claude-code'],
            agents: ['qwen-code'],
          })}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
            ['gemini-cli', 'Gemini'],
            ['qwen-code', 'Qwen'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('Claude Code')).toBeTruthy();
    expect(screen.getByText('Codex')).toBeTruthy();
    expect(screen.getByText('Gemini')).toBeTruthy();
    expect(screen.queryByText('Qwen')).toBeNull();
  });

  it('does not fall back to skill agents when summary arrays are present but empty', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            defaultAvailableAgents: [],
            privateAdaptedAgents: [],
            privateCopyAgents: [],
            agents: ['claude-code'],
          })}
          displayScope="global"
          agentDisplayNames={new Map([['claude-code', 'Claude Code']])}
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('Claude Code')).toBeNull();
  });

  it('dedupes duplicate card agent ids before rendering chips', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            associatedAgents: ['claude-code', 'claude-code', 'codex'],
          })}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.getAllByText('Claude Code')).toHaveLength(1);
    expect(screen.getByText('Codex')).toBeTruthy();
  });

  it('does not infer associated Agents when the Backend projection is empty', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            associatedAgents: [],
            agents: ['claude-code', 'codex'],
          })}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('Claude Code')).toBeNull();
    expect(screen.queryByText('Codex')).toBeNull();
  });

  it('does not open details while card text is selected', () => {
    const onClick = vi.fn();
    const getSelectionSpy = vi.spyOn(window, 'getSelection').mockReturnValue({
      toString: () => 'Toolkit',
    } as Selection);

    try {
      render(
        <TooltipProvider>
          <SkillCard
            skill={makeSkill()}
            displayScope="global"
            onClick={onClick}
          />
        </TooltipProvider>
      );

      fireEvent.click(screen.getByText('Toolkit'));

      expect(onClick).not.toHaveBeenCalled();
    } finally {
      getSelectionSpy.mockRestore();
    }
  });

  it('does not open details after dragging across card text', () => {
    const onClick = vi.fn();

    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill()}
          displayScope="global"
          onClick={onClick}
        />
      </TooltipProvider>
    );

    const description = screen.getByText('Toolkit');
    fireEvent.pointerDown(description, { clientX: 10, clientY: 10 });
    fireEvent.click(description, { clientX: 28, clientY: 10 });

    expect(onClick).not.toHaveBeenCalled();
  });

  it('explains an unknown baseline on the source without adding a title status', async () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: false,
              updateReason: 'missingRemoteHash',
              source: 'owner/repo',
              agents: ['claude-code', 'codex'],
              associatedAgents: ['claude-code', 'codex'],
            }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('skills.updateStatusLabel.reinstallRequired')).toBeNull();
    fireEvent.focus(screen.getByText('owner/repo'));
    expect((await screen.findByRole('tooltip')).textContent).toContain('skills.updateHint.missingRemoteHashCanUpdate');
    expect(screen.getByText('Claude Code')).toBeTruthy();
    expect(screen.getByText('Codex')).toBeTruthy();
  });

  it('does not turn a temporary network failure into a permanent card warning', () => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: false,
              updateReason: 'network-error',
              agents: ['claude-code'],
              associatedAgents: ['claude-code'],
            }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
          displayScope="global"
          agentDisplayNames={new Map([['claude-code', 'Claude Code']])}
        />
      </TooltipProvider>
    );

    const agent = screen.getByText('Claude Code');
    expect(screen.queryByTestId('skill-card-attention')).toBeNull();
    expect(agent).toBeTruthy();
  });

  it('keeps a known update available without showing network diagnostics', () => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({ hasUpdate: true, canRunUpdate: true, canCheckForUpdates: true }),
            updateStatus: 'cannotCheck',
            updateReason: 'upstreamUnavailable',
            updateFreshness: 'backingOff',
            updateEvidence: {
              source: 'github.com/owner/repo',
              requestedRef: 'main',
              resolvedRef: 'main',
              refRevision: 'tree-1',
              checkedAtEpochMs: 100,
              expiresAtEpochMs: 200,
              freshness: 'backingOff',
              lastAttempt: {
                checkedAtEpochMs: 300,
                failure: {
                  reason: 'network',
                  message: 'must not be shown',
                  retryAtEpochMs: 500,
                  providerCooldown: false,
                },
              },
            },
          } as never}
          displayScope="global"
          onUpdate={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.queryByTestId('skill-card-attention')).toBeNull();
    expect(screen.getByText('skills.updateStatusLabel.available')).toBeTruthy();
    expect(screen.getByTitle('skills.actions.update')).toBeTruthy();
    expect(screen.queryByText('must not be shown')).toBeNull();
  });

  it('keeps the committed update badge after a failed refresh', () => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({ hasUpdate: true, canRunUpdate: true, canCheckForUpdates: true }),
            updateStatus: 'updateAvailable',
            updateReason: null,
            updateAttempt: { outcome: 'notCompleted', reason: 'upstreamUnavailable' },
            updateEvidence: {
              source: 'github.com/owner/repo',
              requestedRef: 'main',
              resolvedRef: 'main',
              refRevision: 'tree-1',
              checkedAtEpochMs: 100,
              expiresAtEpochMs: 200,
              freshness: 'backingOff',
              lastAttempt: {
                checkedAtEpochMs: 300,
                failure: {
                  reason: 'network',
                  message: 'must not be shown',
                  retryAtEpochMs: 500,
                  providerCooldown: false,
                },
              },
            },
          } as never}
          displayScope="global"
          onUpdate={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateStatusLabel.available')).toBeDefined();
    expect(screen.queryByText('skills.card.updateCheckIncomplete')).toBeNull();
  });

  it('crossfades an execution result into an available update for 160ms', async () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: false,
              updateReason: 'missing-remote-hash',
            }),
            updateStatus: 'cannotCheck',
          } as never}
          updateStatus="done"
          displayScope="global"
        />
      </TooltipProvider>
    );

    rerender(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: true,
              canRunUpdate: true,
              canCheckForUpdates: true,
              updateReason: null,
            }),
            updateStatus: 'updateAvailable',
          } as never}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateDone')).toBeDefined();
    expect(screen.getByText('skills.updateStatusLabel.available')).toBeDefined();

    await act(async () => { await vi.advanceTimersByTimeAsync(160); });
    expect(screen.queryByText('skills.updateDone')).toBeNull();
    vi.useRealTimers();
  });

  it('keeps delete as the only maintenance action for missing version metadata', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: false,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              updateReason: 'missingRemoteHash',
            }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
          displayScope="global"
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getAllByRole('button', { name: /^skills\.actions\./ })).toEqual([
      screen.getByTitle('skills.actions.delete'),
    ]);
  });

  it('shows upstream-deleted state without ordinary update action', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: true,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              updateReason: 'deletedUpstream',
            }),
            updateStatus: 'deletedUpstream',
          } as InstalledSkill & { updateStatus?: 'deletedUpstream' }}
          displayScope="global"
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.card.sourceMissingUpstream')).toBeTruthy();
    expect(screen.queryByText('skills.updateStatusLabel.deletedUpstream')).toBeNull();
    expect(screen.queryByText('skills.updateHint.deletedUpstream')).toBeNull();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
    expect(screen.getByTitle('skills.actions.delete')).toBeTruthy();
  });

  it('hides ordinary update action when update cannot run even if stale update state is present', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: true,
            canRunUpdate: false,
            updateReason: 'missing-skill-path',
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('hides update action for manual-only sources when no update is available', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: false,
            canRunUpdate: true,
            canCheckForUpdates: false,
            updateReason: 'unsupported-source-type',
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('skills.updateStatusLabel.autoCheckUnavailable')).toBeNull();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('shows local sources as local Skills without an attention warning', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: false,
            canRunUpdate: true,
            canCheckForUpdates: false,
            updateReason: 'local-source',
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateStatusLabel.localSource')).toBeTruthy();
    expect(screen.queryByTestId('skill-card-attention')).toBeNull();
    expect(screen.queryByText('skills.updateHint.local-source')).toBeNull();
  });

  it.each([
    ['rate-limited', 'skills.updateHint.rate-limited'],
    ['auth', 'skills.updateHint.auth'],
    ['network-error', 'skills.updateHint.network-error'],
    ['http-404', 'skills.updateHint.http-error'],
  ])('only keeps actionable legacy source issues visible: %s', (reason, expectedKey) => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: false,
            updateReason: reason,
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    if (reason === 'auth') expect(screen.getByText('skills.updateEvidence.failure.authenticationRequired')).toBeTruthy();
    else expect(screen.queryByTestId('skill-card-attention')).toBeNull();
    expect(screen.queryByText(expectedKey)).toBeNull();
  });
});
