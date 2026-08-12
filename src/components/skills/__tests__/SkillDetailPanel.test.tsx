/* @vitest-environment jsdom */

import '@/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { SkillDetailPanel } from '../SkillDetailPanel';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { InstalledSkill } from '@/bindings';
import type { SkillListItem } from '@/stores/skills-utils';
import { useMutationStore } from '@/stores/mutation';

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
  associatedAgents: [],
  hasUpdate: true,
  canCheckForUpdates: true,
  ...overrides,
});

describe('SkillDetailPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.callback = null;
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('disables detail write actions while keeping close available', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'install',
        context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({ scope: 'project', canRunUpdate: true })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onCopyToProject={vi.fn()}
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
    expect((screen.getByTitle('common.close') as HTMLButtonElement).disabled).toBe(false);
  });


  it.each([
    ['acquiring', 'skills.updatePhaseAcquiring'],
    ['validating', 'skills.updatePhaseValidating'],
    ['updating', 'skills.updatePhaseUpdating'],
  ] as const)('shows the %s phase instead of the update button', (updateStatus, label) => {
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
        updateStatus={updateStatus}
      />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
    expect(screen.getByText(label)).toBeTruthy();
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

  it('disables another explicit check while a previous Force request is still pending', () => {
    const onCheckUpdates = vi.fn(async () => 'completed' as const);

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
          onCheckUpdates={onCheckUpdates}
          isCheckingUpdates
        />
      </TooltipProvider>
    );

    const check = screen.getByTitle('skills.checkUpdates');
    expect((check as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(check);
    expect(onCheckUpdates).not.toHaveBeenCalled();
  });

  it('disables Force during provider cooldown and exposes the retry time', () => {
    const retryAtEpochMs = Date.now() + 60_000;
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={{
            ...makeSkill({ hasUpdate: false }),
            updateStatus: 'cannotCheck',
            updateReason: 'upstreamUnavailable',
            updateAttempt: { outcome: 'notCompleted', reason: 'upstreamUnavailable' },
            updateEvidence: {
              source: 'github.com/owner/repo',
              requestedRef: 'main',
              resolvedRef: null,
              refRevision: null,
              checkedAtEpochMs: null,
              expiresAtEpochMs: null,
              freshness: 'coolingDown',
              lastAttempt: {
                checkedAtEpochMs: Date.now(),
                failure: {
                  reason: 'rateLimited',
                  message: 'rate limited',
                  retryAtEpochMs,
                  providerCooldown: true,
                },
              },
            },
          } as never}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onCheckUpdates={vi.fn(async () => 'notCompleted' as const)}
        />
      </TooltipProvider>
    );

    const check = screen.getByTitle('skills.updateEvidence.retryAt') as HTMLButtonElement;
    expect(check.disabled).toBe(true);
  });

  it('keeps Force disabled when another source in the Context established provider cooldown', () => {
    const retryAtEpochMs = Date.now() + 60_000;
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({ source: 'other/repo', hasUpdate: false }) as never}
          sourceDiagnostics={[{
            source: 'github.com/owner/rate-limited',
            requestedRef: 'HEAD',
            resolvedRef: null,
            refRevision: null,
            checkedAtEpochMs: null,
            expiresAtEpochMs: null,
            freshness: 'coolingDown',
            lastAttempt: {
              checkedAtEpochMs: Date.now(),
              failure: {
                reason: 'rateLimited',
                message: 'rate limited',
                retryAtEpochMs,
                providerCooldown: true,
              },
            },
          }]}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onCheckUpdates={vi.fn(async () => 'notCompleted' as const)}
        />
      </TooltipProvider>
    );

    expect((screen.getByTitle('skills.updateEvidence.retryAt') as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it('re-enables Force when the observed provider cooldown is already expired', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    const retryAtEpochMs = 1_060_000;
    const skill: SkillListItem = {
      ...makeSkill({ hasUpdate: false }),
      updateStatus: 'cannotCheck',
      updateReason: 'upstreamUnavailable',
      updateAttempt: { outcome: 'notCompleted', reason: 'upstreamUnavailable' },
      updateEvidence: {
        source: 'github.com/owner/repo',
        requestedRef: 'main',
        resolvedRef: null,
        refRevision: null,
        checkedAtEpochMs: null,
        expiresAtEpochMs: null,
        freshness: 'coolingDown',
        lastAttempt: {
          checkedAtEpochMs: 1_000_000,
          failure: {
            reason: 'rateLimited',
            message: 'rate limited',
            retryAtEpochMs,
            providerCooldown: true,
          },
        },
      },
    };
    const props = {
      skill,
      content: '# Brainstorming',
      loading: false,
      agentDisplayNames: new Map<string, string>(),
      onClose: vi.fn(),
      onUpdate: vi.fn(),
      onDelete: vi.fn(),
      onRetry: vi.fn(),
      onManageAgents: vi.fn(),
      onCheckUpdates: vi.fn(async () => 'notCompleted' as const),
    };
    const { rerender } = render(
      <TooltipProvider>
        <SkillDetailPanel {...props} />
      </TooltipProvider>
    );

    expect((screen.getByTitle('skills.updateEvidence.retryAt') as HTMLButtonElement).disabled).toBe(true);

    vi.setSystemTime(1_120_000);
    rerender(
      <TooltipProvider>
        <SkillDetailPanel
          {...props}
          skill={{
            ...skill,
            updateEvidence: {
              ...skill.updateEvidence!,
              lastAttempt: {
                ...skill.updateEvidence!.lastAttempt!,
                failure: {
                  ...skill.updateEvidence!.lastAttempt!.failure!,
                  retryAtEpochMs: 1_050_000,
                },
              },
            },
          }}
        />
      </TooltipProvider>
    );
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });

    expect((screen.getByTitle('skills.checkUpdates') as HTMLButtonElement).disabled).toBe(false);
    vi.useRealTimers();
  });

  it('shows the latest typed failure, valid evidence time, retry time, and next action', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={{
            ...makeSkill({
              hasUpdate: true,
              canRunUpdate: true,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
            }),
            updateStatus: 'cannotCheck',
            updateReason: 'upstreamUnavailable',
            updateFreshness: 'coolingDown',
            updateEvidence: {
              source: 'github.com/owner/repo',
              requestedRef: 'main',
              resolvedRef: 'main',
              refRevision: 'tree-1',
              checkedAtEpochMs: 1_700_000_000_000,
              expiresAtEpochMs: 1_700_003_600_000,
              freshness: 'coolingDown',
              lastAttempt: {
                checkedAtEpochMs: 1_700_000_100_000,
                failure: {
                  reason: 'rateLimited',
                  message: 'must not be shown',
                  retryAtEpochMs: 1_700_000_200_000,
                  providerCooldown: true,
                },
              },
            },
          } as never}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map()}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          onCheckUpdates={vi.fn(async () => 'notCompleted' as const)}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateStatusLabel.checkIncomplete')).toBeTruthy();
    expect(screen.getByText('skills.updateEvidence.failure.rateLimited')).toBeTruthy();
    expect(screen.getByText('skills.updateEvidence.lastChecked')).toBeTruthy();
    expect(screen.getByText('skills.updateEvidence.lastAttempt')).toBeTruthy();
    expect(screen.getByText('skills.updateEvidence.retryAt')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'skills.updateEvidence.actions.configureToken' }).getAttribute('href'))
      .toBe('/settings?section=git');
    expect(screen.queryByText('must not be shown')).toBeNull();
  });

  it('shows cannotCheck status and reason without exposing update action when no update is available', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
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
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
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
              updateReason: 'missingRemoteHash',
            }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
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
              updateReason: 'deletedUpstream',
            }),
            updateStatus: 'deletedUpstream',
          } as InstalledSkill & { updateStatus?: 'deletedUpstream' }}
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

    expect(screen.getByText('skills.updateStatus.deletedUpstream')).toBeTruthy();
    expect(screen.getByText('skills.updateReason.deletedUpstream')).toBeTruthy();
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
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' }}
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
          onCheckUpdates={vi.fn(async () => 'completed' as const)}
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.checkUpdates')).toBeNull();
  });

  it('shows duplicate copies as a maintenance note instead of another badge group', () => {
    const { rerender } = render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({
            duplicateCopyCount: 2,
            duplicateCopyAgents: ['firebender', 'claude-code'],
          })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map([
            ['firebender', 'Firebender'],
            ['claude-code', 'Claude Code'],
          ])}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.detail.extraCopiesNamedHint')).toBeTruthy();
    expect(screen.queryByText('skills.card.extraCopies')).toBeNull();
    expect(screen.queryByText('skills.detail.duplicateCopiesTitle')).toBeNull();
    expect(screen.queryByText('skills.detail.manageDuplicates')).toBeNull();

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

    expect(screen.queryByText('skills.detail.extraCopiesNamedHint')).toBeNull();
  });

  it('summarizes duplicate copy agents when the maintenance note would be too long', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({
            duplicateCopyCount: 4,
            duplicateCopyAgents: ['codex', 'cursor', 'firebender', 'claude-code'],
          })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map([
            ['codex', 'Codex'],
            ['cursor', 'Cursor'],
            ['firebender', 'Firebender'],
            ['claude-code', 'Claude Code'],
          ])}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.detail.extraCopiesNamedHint')).toBeTruthy();
    expect(screen.getByText('skills.detail.extraCopiesAgentSummaryMore')).toBeTruthy();
    expect(screen.queryByText('Firebender')).toBeNull();
    expect(screen.queryByText('Claude Code')).toBeNull();
  });

  it('shows a stable workflow updating indicator', () => {
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

    expect(screen.getByText('skills.updatePhaseUpdating')).toBeTruthy();
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
          onCheckUpdates={vi.fn(async () => 'completed' as const)}
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
      expect(screen.getByTitle('skills.checkCompleted')).toBeTruthy();
    });

    rerender(
      <TooltipProvider>
        <SkillDetailPanel
          key="global:toolkit"
          skill={makeSkill({ name: 'toolkit', description: 'Toolkit', hasUpdate: false })}
          content="# Toolkit"
          loading={false}
          agentDisplayNames={new Map()}
          onCheckUpdates={vi.fn(async () => 'completed' as const)}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
          isCheckingUpdates={false}
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.checkCompleted')).toBeNull();
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

    expect(screen.getByText('skills.updatePhaseUpdating')).toBeTruthy();

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

    expect(screen.getByText('skills.updatePhaseUpdating')).toBeTruthy();
  });

  it('shows available agent names without technical availability category counts', () => {
    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({
            agents: ['codex', 'cursor', 'firebender'],
            associatedAgents: ['codex', 'cursor'],
            defaultAvailableAgents: ['codex'],
            privateAdaptedAgents: ['cursor'],
            privateCopyAgents: ['firebender'],
          })}
          content="# Brainstorming"
          loading={false}
          agentDisplayNames={new Map([
            ['codex', 'Codex'],
            ['cursor', 'Cursor'],
            ['firebender', 'Firebender'],
          ])}
          onClose={vi.fn()}
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('Codex')).toBeTruthy();
    expect(screen.getByText('Cursor')).toBeTruthy();
    expect(screen.queryByText('Firebender')).toBeNull();
    expect(screen.queryByText('skills.detail.defaultAvailableCount')).toBeNull();
    expect(screen.queryByText('skills.detail.privateAdaptedCount')).toBeNull();
    expect(screen.queryByText('skills.detail.privateCopyCount')).toBeNull();
  });
});
