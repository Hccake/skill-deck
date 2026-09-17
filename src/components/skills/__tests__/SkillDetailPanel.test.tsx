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
    t: (key: string, values?: { path?: string }) => values?.path ? `${key}:${values.path}` : key,
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

const makeSkill = (overrides: Partial<SkillListItem> = {}): SkillListItem => ({
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
  it('does not call a completed check up to date when the source Skill was deleted', async () => {
    render(<TooltipProvider><SkillDetailPanel skill={makeSkill({ hasUpdate: false, updateStatus: 'deletedUpstream', updateReason: 'deletedUpstream' })}
      content="Content" loading={false} agentDisplayNames={new Map()} onClose={vi.fn()}
      onUpdate={vi.fn()} onDelete={vi.fn()} onRetry={vi.fn()} onManageAgents={vi.fn()}
      onCheckUpdates={vi.fn(async () => 'completed' as const)} /></TooltipProvider>);
    await act(async () => { fireEvent.click(screen.getByTitle('skills.checkUpdates')); });
    expect(screen.queryByTitle('skills.checkCompleted')).toBeNull();
    expect(screen.queryByTitle('skills.checkUpToDate')).toBeNull();
    expect(screen.getByText('skills.card.sourceMissingUpstream')).toBeTruthy();
  });
  it('keeps an update entry for remaining copies after the source is already current', () => {
    const onUpdate = vi.fn();
    render(<TooltipProvider><SkillDetailPanel skill={makeSkill({ hasUpdate: false, canRunUpdate: true })}
      content="Content" loading={false} agentDisplayNames={new Map()} onClose={vi.fn()}
      onUpdate={onUpdate} onDelete={vi.fn()} onRetry={vi.fn()} onManageAgents={vi.fn()} /></TooltipProvider>);
    fireEvent.click(screen.getByTitle('skills.actions.update'));
    expect(onUpdate).toHaveBeenCalledWith('brainstorming', 'global');
  });
  it('shows the same-name library version with its own maintenance entry', () => {
    const version = { libraryId: 'team', libraryName: 'Team Skills', skillName: 'brainstorming' };
    const onOpenLibraryVersion = vi.fn();
    render(<TooltipProvider><SkillDetailPanel
      skill={makeSkill({ libraryVersions: [version] })} content="Direct content" loading={false}
      agentDisplayNames={new Map()} onClose={vi.fn()} onUpdate={vi.fn()} onDelete={vi.fn()}
      onRetry={vi.fn()} onManageAgents={vi.fn()} onOpenLibraryVersion={onOpenLibraryVersion}
    /></TooltipProvider>);
    fireEvent.click(screen.getByRole('button', { name: /Team Skills/ }));
    expect(onOpenLibraryVersion).toHaveBeenCalledWith(version);
    expect(screen.getByText('Direct content')).toBeTruthy();
  });

  it('explains an unsupported Eve file and disables maintenance writes', () => {
    render(<TooltipProvider><SkillDetailPanel
      skill={makeSkill({ maintenanceError: { kind: 'capabilityUnavailable', data: { capability: 'eveSingleFile', path: '/project/agent/skills/demo.md' } } })}
      content={null} loading={false} agentDisplayNames={new Map()} onClose={vi.fn()} onUpdate={vi.fn()}
      onDelete={vi.fn()} onRetry={vi.fn()} onManageAgents={vi.fn()}
    /></TooltipProvider>);
    expect(screen.getByRole('alert').textContent).toContain('mutation.result.errors.eveSingleFile');
    expect((screen.getByTitle('skills.manageAgents.action') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTitle('skills.actions.delete') as HTMLButtonElement).disabled).toBe(true);
  });

  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.callback = null;
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('shows a scoped install path and reveals its full address on demand', async () => {
    const canonicalPath = 'C:\\Users\\cheng\\AppData\\Roaming\\Skill Deck\\skills\\a-very-long-skill-name';
    const root = { environment: { kind: 'native' as const }, nativePath: 'C:\\Users\\cheng' };

    render(
      <TooltipProvider>
        <SkillDetailPanel
          skill={makeSkill({ canonicalPath })}
          context={{ environment: root.environment, scope: { scope: 'global' } }}
          pathBase={{ logicalRoot: root, physicalRoot: root, pathStyle: 'windows' }}
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

    const path = screen.getByRole('button', { name: 'skills.installPath.viewFullPath:~\\AppData\\Roaming\\Skill Deck\\skills\\a-very-long-skill-name' });
    expect(screen.queryByText(canonicalPath)).toBeNull();
    fireEvent.focus(path);
    expect((await screen.findByRole('tooltip')).textContent).toContain(canonicalPath);
  });

  it('disables detail write actions while keeping close available', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'install',
        target: { kind: 'skillLocation', environment: { kind: 'native' }, scope: { scope: 'global' } },
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

  it('keeps update and credential actions without expanding rate-limit diagnostics', () => {
    const onConfigureGitCredentials = vi.fn();
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
          onConfigureGitCredentials={onConfigureGitCredentials}
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

    expect(screen.getByText('skills.updateStatusLabel.available')).toBeTruthy();
    expect(screen.queryByText('skills.updateStatusLabel.checkIncomplete')).toBeNull();
    expect(screen.queryByText('skills.updateEvidence.lastChecked')).toBeNull();
    expect(screen.queryByText('skills.updateEvidence.lastAttempt')).toBeNull();
    // 桌面应用里 <a href> 会整页重载，配置凭据改为由页面提供的路由回调。
    const configureToken = screen.getByRole('button', {
      name: 'skills.updateEvidence.actions.configureToken',
    });
    expect(screen.queryByRole('link', { name: 'skills.updateEvidence.actions.configureToken' })).toBeNull();
    fireEvent.click(configureToken);
    expect(onConfigureGitCredentials).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('must not be shown')).toBeNull();
  });

  it('shows cannotCheck reason while preserving a backend-authorized manual update', () => {
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

    expect(screen.getByText('skills.card.sourceIncomplete')).toBeTruthy();
    expect(screen.queryByText('skills.updateStatus.cannotCheck')).toBeNull();
    expect(screen.getByTitle('skills.actions.update')).toBeTruthy();
  });

  it('shows upstream-deleted state without ordinary update action', () => {
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
          onUpdate={vi.fn()}
          onDelete={vi.fn()}
          onRetry={vi.fn()}
          onManageAgents={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.card.sourceMissingUpstream')).toBeTruthy();
    expect(screen.queryByText('skills.updateReason.deletedUpstream')).toBeNull();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
    expect(screen.getByTitle('skills.actions.delete')).toBeTruthy();
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

  it('keeps manual update available when the backend cannot compare versions', () => {
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

    expect(screen.getByTitle('skills.actions.update')).toBeTruthy();
  });

  it.each([
    ['rate-limited', 'skills.updateReason.rate-limited'],
    ['auth', 'skills.updateReason.auth'],
    ['network-error', 'skills.updateReason.network-error'],
    ['http-404', 'skills.updateReason.http-error'],
  ])('keeps legacy diagnostics compact: %s', (reason, expectedKey) => {
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

    expect(screen.queryByText(expectedKey)).toBeNull();
    if (reason === 'auth') expect(screen.getByText('skills.updateEvidence.failure.authenticationRequired')).toBeTruthy();
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
          skill={makeSkill({ name: 'brainstorming', hasUpdate: false, updateStatus: 'upToDate' })}
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
      expect(screen.getByTitle('skills.checkUpToDate')).toBeTruthy();
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

    expect(screen.queryByTitle('skills.checkUpToDate')).toBeNull();
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
