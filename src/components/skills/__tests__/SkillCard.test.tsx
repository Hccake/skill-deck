/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { SkillCard } from '../SkillCard';
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

    for (const title of [
      'skills.actions.update',
      'skills.actions.copyToProject',
      'skills.manageAgents.action',
      'skills.actions.delete',
    ]) {
      expect((screen.getByTitle(title) as HTMLButtonElement).disabled).toBe(true);
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

  it('shows cannotCheck status in the title row when no update is available', () => {
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

    expect(screen.getByText('skills.updateStatusLabel.needsSourceInfo')).toBeTruthy();
    expect(screen.getByText('skills.updateHint.missing-skill-path')).toBeTruthy();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('keeps the single skill update action as the primary action when an update is available', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: true,
            canRunUpdate: true,
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.getByTitle('skills.actions.update')).toBeTruthy();
    expect(screen.getByText('skills.updateStatusLabel.available')).toBeTruthy();
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

  it('shows extra-copy maintenance as a warning icon only when duplicate copies are reported', async () => {
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
    expect(screen.queryByText('skills.card.extraCopies')).toBeNull();

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

    expect(screen.queryByText('skills.card.extraCopies')).toBeNull();
    const extraCopiesIcon = screen.getByLabelText('skills.card.extraCopies');
    expect(extraCopiesIcon).toBeTruthy();
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    await userEvent.hover(extraCopiesIcon);
    const tooltips = await screen.findAllByTestId('skill-card-extra-copies-tooltip');
    expect(tooltips.some((tooltip) => tooltip.textContent === 'skills.card.extraCopiesHint'))
      .toBe(true);
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

  it('shows update diagnostics as metadata before agent badges', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: false,
              updateReason: 'missingRemoteHash',
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

    expect(screen.getByText('skills.updateStatusLabel.reinstallRequired')).toBeTruthy();
    const diagnostic = screen.getByText('skills.updateHint.missingRemoteHash');
    const firstAgent = screen.getByText('Claude Code');

    expect(diagnostic).toBeTruthy();
    expect(
      diagnostic.compareDocumentPosition(firstAgent) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(screen.getByText('Codex')).toBeTruthy();
  });

  it('shows a temporary update failure reason from the focusable status label', async () => {
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

    const updateBadge = screen.getByText('skills.updateStatusLabel.checkFailed');
    const agent = screen.getByText('Claude Code');

    expect(screen.queryByText('skills.updateHint.network-error')).toBeNull();
    expect(updateBadge.getAttribute('tabindex')).toBe('0');
    expect(agent).toBeTruthy();

    fireEvent.focus(updateBadge);
    expect((await screen.findByRole('tooltip')).textContent).toContain('skills.updateHint.network-error');
  });

  it('shows typed check diagnostics while keeping the last known update available', async () => {
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

    const status = screen.getByText('skills.updateStatusLabel.checkIncomplete');
    expect(screen.getByTitle('skills.actions.update')).toBeTruthy();
    fireEvent.focus(status);
    const tooltip = await screen.findByRole('tooltip');
    expect(tooltip.textContent).toContain('skills.updateEvidence.failure.network');
    expect(tooltip.textContent).toContain('skills.updateEvidence.nextStep.retry');
    expect(tooltip.textContent).not.toContain('must not be shown');
  });

  it('keeps the committed update badge and adds a separate warning after a failed refresh', async () => {
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
    expect(screen.getByTestId('skill-update-warning')).toBeDefined();
  });

  it('crossfades a changed card update status for 160ms', async () => {
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

    expect(screen.getByText('skills.updateStatusLabel.reinstallRequired')).toBeDefined();
    expect(screen.getByText('skills.updateStatusLabel.available')).toBeDefined();

    await act(async () => { await vi.advanceTimersByTimeAsync(160); });
    expect(screen.queryByText('skills.updateStatusLabel.reinstallRequired')).toBeNull();
    vi.useRealTimers();
  });

  it('shows repair source action for missing skill path metadata', () => {
    const onRepairSource = vi.fn();

    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: false,
              canCheckForUpdates: false,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
          displayScope="global"
          onRepairSource={onRepairSource}
        />
      </TooltipProvider>
    );

    const repairAction = screen.getByTitle('skills.actions.repairSource');

    fireEvent.click(repairAction);

    expect(onRepairSource).toHaveBeenCalledWith(expect.objectContaining({ name: 'toolkit' }));
  });

  it('uses direct reinstall for missing version metadata', () => {
    const onUpdate = vi.fn();
    const onRepairSource = vi.fn();

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
          onUpdate={onUpdate}
          onRepairSource={onRepairSource}
        />
      </TooltipProvider>
    );

    const reinstallAction = screen.getByTitle('skills.actions.reinstall');

    fireEvent.click(reinstallAction);

    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByText('skills.reinstallConfirm.title')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'skills.reinstallConfirm.confirm' }));

    expect(onUpdate).toHaveBeenCalledWith('toolkit');
    expect(onRepairSource).not.toHaveBeenCalled();
  });

  it('shows upstream-deleted state without ordinary update action', () => {
    const onUpdate = vi.fn();
    const onRepairSource = vi.fn();

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
          onUpdate={onUpdate}
          onRepairSource={onRepairSource}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateStatusLabel.deletedUpstream')).toBeTruthy();
    expect(screen.getByText('skills.updateHint.deletedUpstream')).toBeTruthy();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();

    fireEvent.click(screen.getByTitle('skills.updatePlan.deletedUpstreamActionRepair'));

    expect(onRepairSource).toHaveBeenCalledWith(expect.objectContaining({ name: 'toolkit' }));
    expect(onUpdate).not.toHaveBeenCalled();
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

    expect(screen.getByText('skills.updateStatusLabel.autoCheckUnavailable')).toBeDefined();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('uses auto-check unavailable copy for local sources', () => {
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

    expect(screen.getByText('skills.updateStatusLabel.autoCheckUnavailable')).toBeTruthy();
    expect(screen.getByText('skills.updateHint.local-source')).toBeTruthy();
  });

  it.each([
    ['rate-limited', 'skills.updateHint.rate-limited'],
    ['auth', 'skills.updateHint.auth'],
    ['network-error', 'skills.updateHint.network-error'],
    ['http-404', 'skills.updateHint.http-error'],
  ])('shows GitHub update reason %s', (reason, expectedKey) => {
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

    expect(screen.getByText('skills.updateStatusLabel.checkFailed')).toBeTruthy();
    expect(screen.queryByText(expectedKey)).toBeNull();
  });
});
