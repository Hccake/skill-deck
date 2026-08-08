/* @vitest-environment jsdom */

import '@/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render as testingRender, screen, waitFor } from '@testing-library/react';
import { SkillsSection } from '../SkillsSection';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { InstalledSkill } from '@/bindings';
import type { SkillListItem } from '@/stores/skills-utils';
import { useMutationStore } from '@/stores/mutation';

const render = (ui: Parameters<typeof testingRender>[0]) => (
  testingRender(ui, { wrapper: TooltipProvider })
);

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en' },
  }),
}));

vi.mock('../SkillCard', () => ({
  SkillCard: ({
    skill,
    updateStatus,
    onUpdate,
    onRepairSource,
  }: {
    skill: InstalledSkill;
    updateStatus?: 'acquiring' | 'validating' | 'updating' | 'done' | 'failed';
    onUpdate?: (skillName: string) => void;
    onRepairSource?: (skill: InstalledSkill) => void;
  }) => (
    <div data-testid={`skill-card:${skill.scope}:${skill.name}`}>
      <span data-testid={`status:${skill.scope}:${skill.name}`}>{updateStatus ?? 'idle'}</span>
      <button type="button" data-testid={`update:${skill.scope}:${skill.name}`} onClick={() => onUpdate?.(skill.name)}>
        update
      </button>
      <button type="button" data-testid={`repair:${skill.scope}:${skill.name}`} onClick={() => onRepairSource?.(skill)}>
        repair
      </button>
    </div>
  ),
}));

const makeSkill = (
  scope: 'global' | 'project',
  overrides: Partial<SkillListItem> = {},
): SkillListItem => ({
  name: 'toolkit',
  description: '',
  path: `/skills/${scope}/toolkit`,
  canonicalPath: `/canonical/${scope}/toolkit`,
  scope,
  agents: [],
  associatedAgents: [],
  hasUpdate: true,
  canCheckForUpdates: true,
  ...overrides,
});

describe('SkillsSection', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('keeps the summary blank until a comparison has been committed', () => {
    const props = {
      title: 'Global',
      skills: [makeSkill('global', { hasUpdate: false, updateStatus: 'upToDate' })],
      scope: 'global' as const,
      updatingSkills: new Map<string, never>(),
      onSkillClick: vi.fn(),
      onPrepareUpdate: vi.fn(async () => true),
      onDelete: vi.fn(),
      onAdd: vi.fn(),
    };
    const { rerender } = render(<SkillsSection {...props} />);
    expect(screen.queryByText('skills.upToDate')).toBeNull();

    rerender(<SkillsSection {...props} hasCommittedComparison />);
    expect(screen.getByText('skills.upToDate')).toBeTruthy();
  });

  it('does not borrow a hidden Skill comparison for an unknown filtered result', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          name: 'unknown',
          hasUpdate: false,
          updateStatus: null,
          updateReason: null,
        })]}
        scope="global"
        updatingSkills={new Map()}
        hasCommittedComparison
        filterActive
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.queryByText('skills.upToDate')).toBeNull();
  });

  it('keeps the last committed summary visible while Automatic is pending', async () => {
    vi.useFakeTimers();
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', { hasUpdate: false, updateStatus: 'upToDate' })]}
        scope="global"
        updatingSkills={new Map()}
        isAutomaticCheckingUpdates
        hasCommittedComparison
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByText('skills.upToDate')).toBeTruthy();
    expect(screen.queryByText('skills.checking')).toBeNull();

    await act(async () => { await vi.advanceTimersByTimeAsync(200); });

    expect(screen.getByText('skills.upToDate')).toBeTruthy();
    expect(screen.getByTestId('update-summary-prefix').querySelector('.animate-spin')).toBeTruthy();
    expect(screen.getByText('skills.checking').className).toContain('sr-only');
  });

  it('keeps the committed summary and adds a warning after a failed refresh', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          hasUpdate: false,
          updateStatus: 'upToDate',
          updateAttempt: { outcome: 'notCompleted', reason: 'upstreamUnavailable' },
          updateEvidence: {
            source: 'github.com/owner/repo',
            requestedRef: 'main',
            resolvedRef: 'main',
            refRevision: 'revision-1',
            checkedAtEpochMs: 100,
            expiresAtEpochMs: 200,
            freshness: 'backingOff',
            lastAttempt: {
              checkedAtEpochMs: 300,
              failure: {
                reason: 'network',
                message: 'offline',
                retryAtEpochMs: 500,
                providerCooldown: false,
              },
            },
          },
        })]}
        scope="global"
        updatingSkills={new Map()}
        hasCommittedComparison
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByText('skills.upToDate')).toBeTruthy();
    expect(screen.queryByText('skills.updateCheckIncompleteCount')).toBeNull();
    const warning = screen.getByLabelText('skills.updateStatusLabel.checkIncomplete');
    expect(warning).toBeTruthy();
    expect(warning.closest('[data-testid="update-summary-slot"]')).toBeTruthy();
    expect(screen.queryByTestId('update-check-progress-slot')).toBeNull();
  });

  it('shows a labelled Automatic status inside the summary only after 200ms', async () => {
    vi.useFakeTimers();
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          hasUpdate: false,
          updateStatus: null,
          updateReason: null,
        })]}
        scope="global"
        updatingSkills={new Map()}
        isAutomaticCheckingUpdates
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.queryByTestId('update-summary-prefix')).toBeNull();
    expect(screen.queryByText('skills.checking')).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(199); });
    expect(screen.queryByTestId('update-summary-prefix')).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    const prefix = screen.getByTestId('update-summary-prefix');
    expect(prefix.querySelector('.animate-spin')).toBeTruthy();
    expect(prefix.closest('[data-testid="update-summary-slot"]')).toBeTruthy();
    expect(screen.getByText('skills.checking')).toBeTruthy();
    expect(screen.queryByTestId('update-check-progress-slot')).toBeNull();
    vi.useRealTimers();
  });

  it('never shows the Automatic spinner when the request finishes before 200ms', async () => {
    vi.useFakeTimers();
    const props = {
      title: 'Global',
      skills: [makeSkill('global')],
      scope: 'global' as const,
      updatingSkills: new Map<string, never>(),
      onSkillClick: vi.fn(),
      onPrepareUpdate: vi.fn(async () => true),
      onDelete: vi.fn(),
      onAdd: vi.fn(),
    };
    const { rerender } = render(<SkillsSection {...props} />);

    rerender(<SkillsSection {...props} isAutomaticCheckingUpdates />);
    await act(async () => { await vi.advanceTimersByTimeAsync(150); });
    rerender(<SkillsSection {...props} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });

    expect(screen.getByTestId('update-summary-prefix').querySelector('.animate-spin')).toBeNull();
    expect(screen.queryByText('skills.checking')).toBeNull();
    vi.useRealTimers();
  });

  it('keeps static maintenance summary stable before, during, and after Automatic checking', () => {
    const props = {
      title: 'Global',
      skills: [
        makeSkill('global', {
          name: 'legacy',
          hasUpdate: false,
          canRunUpdate: true,
          canCheckForUpdates: false,
          updateStatus: 'cannotCheck',
          updateReason: 'missing-remote-hash',
        }),
        makeSkill('global', {
          name: 'eligible',
          hasUpdate: false,
          updateStatus: 'upToDate',
        }),
      ],
      scope: 'global' as const,
      updatingSkills: new Map<string, never>(),
      hasCommittedComparison: true,
      onSkillClick: vi.fn(),
      onPrepareUpdate: vi.fn(async () => true),
      onDelete: vi.fn(),
      onAdd: vi.fn(),
    };
    const { rerender } = render(<SkillsSection {...props} />);
    const summary = screen.getByText('skills.uncheckableUpdateCount');

    rerender(<SkillsSection {...props} isAutomaticCheckingUpdates />);
    expect(screen.getByText('skills.uncheckableUpdateCount')).toBe(summary);
    expect(screen.queryByText('skills.checking')).toBeNull();

    rerender(<SkillsSection {...props} />);
    expect(screen.getByText('skills.uncheckableUpdateCount')).toBe(summary);
  });

  it('crossfades changed polite live-region content and removes the outgoing summary after 160ms', async () => {
    vi.useFakeTimers();
    const props = {
      title: 'Global',
      scope: 'global' as const,
      updatingSkills: new Map<string, never>(),
      hasCommittedComparison: true,
      onSkillClick: vi.fn(),
      onPrepareUpdate: vi.fn(async () => true),
      onDelete: vi.fn(),
      onAdd: vi.fn(),
    };
    const upToDateSkill = makeSkill('global', { hasUpdate: false, updateStatus: 'upToDate' });
    const { rerender } = render(<SkillsSection {...props} skills={[upToDateSkill]} />);
    const liveRegion = screen.getByTestId('update-summary-slot');
    const initialSummary = screen.getByText('skills.upToDate');

    expect(liveRegion.getAttribute('aria-live')).toBe('polite');
    expect(initialSummary.closest('[data-crossfade-state="current"]')?.className).toContain('fade-in');

    rerender(
      <SkillsSection
        {...props}
        skills={[upToDateSkill]}
        isAutomaticCheckingUpdates
      />
    );
    expect(screen.getByText('skills.upToDate')).toBe(initialSummary);

    rerender(
      <SkillsSection
        {...props}
        skills={[makeSkill('global', {
          hasUpdate: true,
          canRunUpdate: true,
          updateStatus: 'updateAvailable',
        })]}
      />
    );
    const nextSummary = screen.getByText('1 skills.update');
    const outgoing = screen.getByText('skills.upToDate').closest('[data-crossfade-state="outgoing"]');
    const current = nextSummary.closest('[data-crossfade-state="current"]');

    expect(outgoing?.className).toContain('fade-out');
    expect(outgoing?.className).toContain('duration-[160ms]');
    expect(outgoing?.className).toContain('motion-reduce:hidden');
    expect(current?.className).toContain('fade-in');
    expect(current?.className).toContain('duration-[160ms]');
    expect(current?.className).toContain('motion-reduce:animate-none');

    await act(async () => { await vi.advanceTimersByTimeAsync(160); });
    expect(screen.queryByText('skills.upToDate')).toBeNull();
    vi.useRealTimers();
  });

  it('reports dynamic incomplete checks and static uncheckable Skills separately', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            name: 'offline',
            hasUpdate: false,
            canCheckForUpdates: true,
            updateStatus: 'cannotCheck',
            updateReason: 'network-error',
          }),
          makeSkill('global', {
            name: 'legacy',
            hasUpdate: false,
            canRunUpdate: true,
            canCheckForUpdates: false,
            updateStatus: 'cannotCheck',
            updateReason: 'missing-remote-hash',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByText('skills.updateCheckIncompleteCount')).toBeTruthy();
    expect(screen.getByText('skills.uncheckableUpdateCount')).toBeTruthy();

    const summary = screen.getByTestId('update-summary-slot');
    const identity = summary.parentElement;
    const header = identity?.parentElement;
    const actions = screen.getByTestId('skills-section-actions');

    expect(header?.className).toContain('flex-row');
    expect(header?.className).not.toContain('flex-col');
    expect(identity?.className).toContain('min-w-0');
    expect(identity?.className).not.toContain('flex-wrap');
    expect(summary.className).toContain('h-10');
    expect(summary.className).toContain('min-w-0');
    expect(summary.className).toContain('flex-1');
    expect(summary.className).not.toContain('w-72');
    expect(summary.className).not.toContain('shrink-0');
    expect(actions.className).toContain('shrink-0');
    expect(screen.queryByTestId('update-check-progress-slot')).toBeNull();
    expect(screen.getAllByTestId('update-summary-prefix')).toHaveLength(2);
  });

  it('disables Force during provider cooldown and exposes the retry time', () => {
    const retryAtEpochMs = Date.now() + 60_000;
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          hasUpdate: false,
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
        })]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={async () => 'notCompleted' as const}
      />
    );

    const button = screen.getByRole('button', { name: 'skills.checkUpdates' }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(button.title).toContain('skills.updateEvidence.retryAt');
    expect(screen.getByText('skills.updateCheckIncompleteCount')).toBeTruthy();
  });

  it('keeps Force disabled when filtering hides the source that established provider cooldown', () => {
    const retryAtEpochMs = Date.now() + 60_000;
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', { source: 'other/repo', hasUpdate: false })]}
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
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={async () => 'notCompleted' as const}
      />
    );

    expect((screen.getByRole('button', { name: 'skills.checkUpdates' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it('re-enables Force when the observed provider cooldown is already expired', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    const retryAtEpochMs = 1_060_000;
    const skill = makeSkill('global', {
      hasUpdate: false,
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
    });
    const props = {
      title: 'Global',
      skills: [skill],
      scope: 'global' as const,
      updatingSkills: new Map<string, never>(),
      onSkillClick: vi.fn(),
      onPrepareUpdate: vi.fn(async () => true),
      onDelete: vi.fn(),
      onAdd: vi.fn(),
      onCheckUpdates: vi.fn(async () => 'notCompleted' as const),
    };
    const { rerender } = render(<SkillsSection {...props} />);

    expect((screen.getByRole('button', { name: 'skills.checkUpdates' }) as HTMLButtonElement).disabled).toBe(true);

    vi.setSystemTime(1_120_000);
    rerender(<SkillsSection
      {...props}
      skills={[{
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
      }]}
    />);
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });

    expect((screen.getByRole('button', { name: 'skills.checkUpdates' }) as HTMLButtonElement).disabled).toBe(false);
    vi.useRealTimers();
  });

  it('disables write actions but keeps update checks available during another mutation', () => {
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
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    expect((screen.getByRole('button', { name: 'skills.updateAll' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'skills.add' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'skills.checkUpdates' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('reads update state using the full skill identity key', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map([['global:toolkit', 'updating']])}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByTestId('status:global:toolkit').textContent).toBe('updating');
  });

  it('does not show a completed check state after external polling finishes', async () => {
    const { rerender } = render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    rerender(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    rerender(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    await waitFor(() => {
      expect(screen.queryByText('skills.updateDone')).toBeNull();
    });
  });

  it('marks the manual check as busy and respects reduced motion', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    const button = screen.getByRole('button', { name: 'skills.checkUpdates' });
    const spinner = button.querySelector('.animate-spin');

    expect(button.getAttribute('aria-busy')).toBe('true');
    expect(spinner?.classList.contains('motion-reduce:animate-none')).toBe(true);
  });

  it('shows a completed check state without describing the Skill as updated', async () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', { hasUpdate: false })]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    fireEvent.click(screen.getByText('skills.checkUpdates'));

    await waitFor(() => {
      expect(screen.getByText('skills.checkCompleted')).toBeTruthy();
      expect(screen.queryByText('skills.updateDone')).toBeNull();
    });
  });

  it('does not show a completed state when an explicit check is partial', async () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', { hasUpdate: false, updateStatus: 'cannotCheck', updateReason: 'upstreamUnavailable' })]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'partial' as const)}
      />
    );

    fireEvent.click(screen.getByText('skills.checkUpdates'));

    await waitFor(() => {
      expect(screen.queryByText('skills.checkCompleted')).toBeNull();
    });
  });

  it('shows an inaccessible project as a neutral empty state without actions', () => {
    render(
      <SkillsSection
        title="Project"
        skills={[]}
        scope="project"
        pathExists={false}
        projectPath="D:\\Code\\project-a"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    const unavailable = screen.getByRole('status', {
      name: 'skills.projectUnavailableTitle',
    });

    expect(screen.getByText('skills.projectUnavailableDescription')).toBeTruthy();
    expect(screen.queryByText('skills.projectNotFound')).toBeNull();
    expect(screen.queryByText('skills.upToDate')).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
    expect(unavailable.className).toContain('border-dashed');
    expect(unavailable.className).not.toContain('border-l-');
    expect(unavailable.className).not.toContain('warning');
    expect(unavailable.className).not.toContain('amber');
  });

  it('does not report the filtered result as up to date', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[]}
        scope="global"
        filterActive
        updatingSkills={new Map()}
        emptyState={<div>filtered-empty</div>}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    expect(screen.getByText('filtered-empty')).toBeDefined();
    expect(screen.queryByText('skills.upToDate')).toBeNull();
  });

  it('summarizes failed Skill checks without exposing source diagnostics', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            name: 'toolkit',
            hasUpdate: false,
            canCheckForUpdates: true,
            updateStatus: 'cannotCheck',
            updateReason: 'rate-limited',
          }),
          makeSkill('global', {
            name: 'writer',
            hasUpdate: false,
            canCheckForUpdates: true,
            updateStatus: 'cannotCheck',
            updateReason: 'network-error',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    expect(screen.getByText('skills.updateCheckIncompleteCount')).toBeTruthy();
    expect(screen.queryByText('skills.uncheckableUpdateCount')).toBeNull();
    expect(screen.queryByText('skills.upToDate')).toBeNull();
    expect(screen.queryByText('github.com/owner/stale')).toBeNull();
    expect(screen.queryByText('github.com/owner/cooling-down')).toBeNull();
    expect(screen.queryByLabelText('skills.updateEvidence.title')).toBeNull();
  });

  it('keeps the update check action available when the backend reports a cooling state', async () => {
    const onCheckUpdates = vi.fn(async () => 'completed' as const);
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          hasUpdate: false,
          updateStatus: 'cannotCheck',
          updateReason: 'rate-limited',
        })]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={onCheckUpdates}
      />
    );

    const checkButton = screen.getByRole('button', { name: 'skills.checkUpdates' });
    expect(checkButton.getAttribute('aria-disabled')).toBeNull();
    fireEvent.click(checkButton);

    await waitFor(() => {
      expect(onCheckUpdates).toHaveBeenCalledTimes(1);
    });
  });

  it('hides the check-updates action when no skills in the section can be checked', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          {
            ...makeSkill('global', { hasUpdate: false, canCheckForUpdates: false }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' },
        ]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    expect(screen.queryByText('skills.checkUpdates')).toBeNull();
  });

  it('hides the check-updates action when capability metadata is missing', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', { hasUpdate: false, canCheckForUpdates: undefined })]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    expect(screen.queryByText('skills.checkUpdates')).toBeNull();
  });

  it('passes repair source actions to skill cards', () => {
    const onRepairSource = vi.fn();

    render(
      <SkillsSection
        title="Project"
        skills={[makeSkill('project', { hasUpdate: false, updateReason: 'missing-skill-path' })]}
        scope="project"
        projectPath="D:\\Code\\project-a"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onRepairSource={onRepairSource}
        onAdd={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId('repair:project:toolkit'));

    expect(onRepairSource).toHaveBeenCalledWith(expect.objectContaining({ scope: 'project', name: 'toolkit' }));
  });

  it('delegates update-all preview to the page-level update workflow owner', async () => {
    const onPrepareUpdate = vi.fn(async () => true);

    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            hasUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={onPrepareUpdate}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    fireEvent.click(screen.getByText('skills.updateAll'));

    await waitFor(() => {
      expect(onPrepareUpdate).toHaveBeenCalledWith(['toolkit'], true);
    });
    expect(screen.queryByText('skills.updatePlan.readyTitle')).toBeNull();
  });

  it('delegates a direct reinstall without a remote hash to the update workflow', async () => {
    const onPrepareUpdate = vi.fn(async () => true);

    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          hasUpdate: false,
          canRunUpdate: true,
          canCheckForUpdates: false,
          updateStatus: 'cannotCheck',
          updateReason: 'missingRemoteHash',
        } as Partial<InstalledSkill>)]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={onPrepareUpdate}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId('update:global:toolkit'));

    await waitFor(() => {
      expect(onPrepareUpdate).toHaveBeenCalledWith(['toolkit'], false);
    });
  });

  it('treats the legacy missing-remote-hash lock reason as static reinstall maintenance', async () => {
    const onPrepareUpdate = vi.fn(async () => true);

    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          hasUpdate: false,
          canRunUpdate: true,
          canCheckForUpdates: false,
          updateStatus: 'cannotCheck',
          updateReason: 'missing-remote-hash',
        } as Partial<InstalledSkill>)]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={onPrepareUpdate}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByText('skills.uncheckableUpdateCount')).toBeTruthy();
    expect(screen.queryByText('skills.updateCheckIncompleteCount')).toBeNull();
    fireEvent.click(screen.getByTestId('update:global:toolkit'));

    await waitFor(() => {
      expect(onPrepareUpdate).toHaveBeenCalledWith(['toolkit'], false);
    });
  });

  it('renders update all as a unified secondary action when updates are available', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            hasUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => 'completed' as const)}
      />
    );

    const actions = screen.getByTestId('skills-section-actions');
    const secondaryActions = screen.getByTestId('skills-section-secondary-actions');
    const updateAll = screen.getByRole('button', { name: 'skills.updateAll' });
    const checkUpdates = screen.getByRole('button', { name: 'skills.checkUpdates' });

    expect(actions.contains(updateAll)).toBe(true);
    expect(secondaryActions.contains(updateAll)).toBe(true);
    expect(secondaryActions.contains(checkUpdates)).toBe(true);
    expect(actions.className).toContain('gap-2');
    expect(secondaryActions.className).toContain('gap-0.5');
    expect(updateAll.className).toContain('h-7');
    expect(updateAll.className).toContain('px-2');
    expect(updateAll.className).toContain('text-muted-foreground');
    expect(updateAll.getAttribute('data-variant')).toBe('ghost');
    expect(updateAll.className).not.toContain('h-auto');
    expect(updateAll.className).not.toContain('p-0');
    expect(updateAll.className).not.toContain('border-primary');
  });

  it('uses neutral summary styling for available update counts', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            hasUpdate: true,
            canRunUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    const updateCount = screen.getByText('1 skills.update');

    expect(updateCount.className).not.toContain('text-warning');
    expect(updateCount.className).toContain('text-muted-foreground');
  });

  it('does not show update all when the section only has maintenance items', () => {
    render(
      <SkillsSection
        title="Project"
        skills={[
          makeSkill('project', {
            hasUpdate: false,
            canRunUpdate: false,
            canCheckForUpdates: false,
            updateReason: 'missing-skill-path',
            updateStatus: 'cannotCheck',
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          } as Partial<InstalledSkill>),
        ]}
        scope="project"
        projectPath="D:\\Code\\project-a"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onRepairSource={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.queryByText('skills.updateAll')).toBeNull();
    expect(screen.getByText('skills.uncheckableUpdateCount')).toBeTruthy();
    expect(screen.queryByText('skills.maintenanceNotice')).toBeNull();
  });

  it('shows update all only for directly updatable skills', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            hasUpdate: true,
            canRunUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
          makeSkill('global', {
            name: 'legacy-toolkit',
            hasUpdate: false,
            canRunUpdate: false,
            canCheckForUpdates: false,
            updateStatus: 'cannotCheck',
            updateReason: 'missing-skill-path',
          } as Partial<InstalledSkill>),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByText('skills.updateAll')).toBeTruthy();
    expect(screen.getByText('skills.uncheckableUpdateCount')).toBeTruthy();
    expect(screen.queryByText('skills.maintenanceNotice')).toBeNull();
  });
});
