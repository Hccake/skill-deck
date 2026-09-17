/* @vitest-environment jsdom */
import '@/test-utils';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { UpdatePlanDialog } from '../UpdatePlanDialog';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useMutationStore } from '@/stores/mutation';
import type { ActiveMutation, SkillLocationRef } from '@/bindings';
import { contextKey } from '@/lib/context';

vi.mock('react-i18next', () => ({ useTranslation: () => ({
  t: (key: string, options?: { count?: number; ref?: string; path?: string; skillName?: string }) => {
    if (key === 'skills.updatePlan.readyTitle') return `${key}:${options?.count}`;
    if (key === 'skills.refBadge') return `${key}:${options?.ref}`;
    if (key === 'skills.updatePlan.singleTitle') return `${key}:${options?.skillName}`;
    if (key === 'skills.installPath.viewFullPath') return `${key}:${options?.path}`;
    return key;
  },
}) }));

const context: SkillLocationRef = { environment: { kind: 'native' }, scope: { scope: 'global' } };
const target = { kind: 'skillLocation' as const, environment: context.environment, scope: context.scope };

const automaticTarget = (nativePath = '/canonical/toolkit') => ({
  displayPath: { environment: context.environment, nativePath },
  readers: [],
  restoring: false,
});

describe('UpdatePlanDialog', () => {
  beforeEach(() => {
    useSkillUpdateWorkflow.getState().reset();
    useSkillsDataStore.setState({ snapshots: {
      [contextKey(context)]: {
        skills: [{ name: 'toolkit', description: '', path: '/skills/toolkit', canonicalPath: '/canonical/toolkit', scope: 'global', agents: [], associatedAgents: [], source: 'owner/repo', hasUpdate: true, canRunUpdate: true, canCheckForUpdates: true, updateStatus: 'updateAvailable', updateReason: null }],
        agents: [], pathExists: true, loading: false, error: null, requestId: 1,
      },
    } });
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('shows every position, keeps fixed targets selected and links optional copies', () => {
    const skill = {
      skillName: 'toolkit', sourceKey: 'source-1', sourceDisplay: 'owner/repo', refDisplay: 'main',
    targets: [{ ...automaticTarget(), isStandard: true, kind: 'directory' as const },
      { ...automaticTarget('/cursor/toolkit'), isStandard: false, kind: 'directory' as const, selectableEntryId: 'cursor-copy',
        readers: [{ agentId: 'cursor', displayName: 'Cursor', logicalTargetId: 'cursor' }] }],
      adapterTargets: [], cleanCopyCount: 0,
      capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
      overwritePrivateEntries: [{ entryId: 'conflict', isStandard: false,
        readers: [{ agentId: 'claude', displayName: 'Claude Code', logicalTargetId: 'claude' }],
        displayPath: { environment: context.environment, nativePath: '/private/toolkit' } }],
      linkedTargets: [{ displayPath: { environment: context.environment, nativePath: '/linked/toolkit' },
        readers: [{ agentId: 'codebuddy', displayName: 'CodeBuddy', logicalTargetId: 'codebuddy' }],
        isStandard: false, targetPath: automaticTarget('/cursor/toolkit').displayPath, targetCopyEntryId: 'cursor-copy' }],
      blockingReasons: [], fallbackForecasts: [],
    };
    useSkillUpdateWorkflow.setState({ phase: 'ready', context, skillNames: ['toolkit', 'other'], selectedCopyEntries: new Set(['cursor-copy']),
      preview: { sources: [], blocked: [], redirectedDownloadHosts: [], skills: [skill,
        { ...skill, skillName: 'other', overwritePrivateEntries: [], linkedTargets: [], targets: [automaticTarget('/canonical/other')] }] },
    });
    render(<UpdatePlanDialog open context={context} skillNames={['toolkit', 'other']} onOpenChange={vi.fn()} />);
    expect(screen.getByRole('heading', { name: 'skills.updatePlan.globalTitle' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'toolkit' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'other' })).toBeTruthy();
    const checkbox = screen.getByRole('checkbox', { name: /toolkit.*Claude Code/ });
    expect(checkbox.getAttribute('aria-checked')).toBe('false');
    const conflict = checkbox.closest('li')!;
    const state = within(conflict).getByText('skills.updatePlan.contentDifferent');
    const kind = within(conflict).getByText('skills.updatePlan.copyKind');
    expect(state.compareDocumentPosition(kind) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'skills.updatePlan.otherLocations' })).toBeNull();
    const footer = document.querySelector('[data-slot="dialog-footer"]')! as HTMLElement;
    expect(within(footer).getByText('skills.updatePlan.preserveConflictDefault')).toBeTruthy();
    expect(within(footer).queryByText('skills.updatePlan.skillCount')).toBeNull();
    const header = document.querySelector('[data-slot="dialog-header"]')! as HTMLElement;
    expect(within(header).queryByText('skills.updatePlan.preserveConflictDefault')).toBeNull();
    expect(screen.getByText('/canonical/toolkit')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'skills.installPath.copyPath' })).toBeNull();
    expect(screen.getByText('CodeBuddy')).toBeTruthy();
    expect(screen.getByText('skills.updatePlan.linkKind')).toBeTruthy();
    const copy = screen.getByRole('checkbox', { name: /toolkit.*Cursor/ });
    const link = screen.getByRole('checkbox', { name: /toolkit.*CodeBuddy/ });
    expect(copy.getAttribute('aria-checked')).toBe('true');
    expect(link.getAttribute('aria-checked')).toBe('true');
    expect((link as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(copy);
    expect(link.getAttribute('aria-checked')).toBe('false');
    fireEvent.click(checkbox);
    expect(screen.getByText('CodeBuddy')).toBeTruthy();
    expect(useSkillUpdateWorkflow.getState().selectedCopyEntries.has('conflict')).toBe(true);
  });

  it('shows a preserved-only Skill without an update or retry action', () => {
    useSkillUpdateWorkflow.setState({ phase: 'ready', context, skillNames: ['toolkit'],
      preview: { sources: [], blocked: [], redirectedDownloadHosts: [], skills: [{
        skillName: 'toolkit', sourceDisplay: 'owner/repo', refDisplay: '', targets: [],
        adapterTargets: [], cleanCopyCount: 0, overwritePrivateEntries: [], linkedTargets: [], fallbackForecasts: [],
        capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
        blockingReasons: ['noUpdateTargets'], preservedTargets: [automaticTarget('/external/toolkit')],
      }] },
    });
    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);
    expect(screen.getByText('skills.updatePlan.noUpdateLocations')).toBeTruthy();
    expect(screen.getAllByRole('button', { name: 'common.close' }).length).toBeGreaterThan(0);
    expect(screen.queryByRole('button', { name: 'skills.updatePlan.confirm' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'common.retry' })).toBeNull();
  });

  it('shows one preparation error per source while keeping every affected Skill visible', () => {
    const error = { kind: 'stalePayload' as const };
    useSkillUpdateWorkflow.setState({ phase: 'ready', context, skillNames: ['alpha', 'beta', 'gamma'],
      preview: {
        sources: [{ sourceKey: 'failed-source', sourceDisplay: 'failed/repo', refDisplay: 'main', skillNames: ['alpha', 'beta'], error },
          { sourceKey: 'ready-source', sourceDisplay: 'ready/repo', refDisplay: 'v2', skillNames: ['gamma'], error: null }],
        blocked: [{ skillName: 'alpha', error }, { skillName: 'beta', error }], redirectedDownloadHosts: [],
        skills: [{ skillName: 'gamma', sourceKey: 'ready-source', sourceDisplay: 'ready/repo', refDisplay: 'v2',
          targets: [automaticTarget('/canonical/gamma')], adapterTargets: [], cleanCopyCount: 0,
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          overwritePrivateEntries: [], blockingReasons: [], linkedTargets: [], fallbackForecasts: [] }],
      },
    });
    render(<UpdatePlanDialog open context={context} skillNames={['alpha', 'beta', 'gamma']} onOpenChange={vi.fn()} />);
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    for (const name of ['alpha', 'beta', 'gamma']) expect(screen.getByRole('heading', { name })).toBeTruthy();
    expect((screen.getByRole('button', { name: 'skills.updatePlan.confirm' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows relative locations and one conflict rule without making path disclosure a selection', async () => {
    const logicalRoot = { environment: context.environment, nativePath: '/home/alice' };
    useSkillUpdateWorkflow.setState({
      phase: 'ready', context, skillNames: ['toolkit'], batch: false,
      preview: {
        pathBase: { logicalRoot, physicalRoot: logicalRoot, pathStyle: 'posix' },
        sources: [], blocked: [], redirectedDownloadHosts: [],
        skills: [{
          skillName: 'toolkit', sourceDisplay: 'owner/repo', refDisplay: 'main',
          targets: [{ ...automaticTarget('/home/alice/.agents/skills/toolkit'), restoring: true }],
          adapterTargets: [], cleanCopyCount: 0,
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          overwritePrivateEntries: ['claude', 'codex'].map((name) => ({
            entryId: name,
            readers: [{ agentId: name, displayName: name, logicalTargetId: name }],
            displayPath: { environment: context.environment, nativePath: `/home/alice/.${name}/skills/toolkit` },
          })),
          blockingReasons: [], linkedTargets: [], fallbackForecasts: [],
        }],
      },
      selectedCopyEntries: new Set(),
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect(screen.getByRole('heading', { name: 'skills.updatePlan.globalTitle' })).not.toBeNull();
    expect(within(screen.getByTestId('update-plan-dialog-body')).getByRole('heading', { name: 'toolkit' })).toBeTruthy();
    expect(screen.getAllByText('skills.updatePlan.preserveConflictDefault')).toHaveLength(1);
    expect(screen.queryByText('context.global')).toBeNull();
    expect(screen.getByText('skills.updatePlan.restoreLocation')).not.toBeNull();
    expect(screen.queryByText('skills.updatePlan.standardSkillAction')).toBeNull();
    const choices = screen.getAllByRole('checkbox');
    expect(choices).toHaveLength(3);
    fireEvent.focus(screen.getByText('~/.claude/skills/toolkit'));
    expect((await screen.findByRole('tooltip')).textContent).toContain('/home/alice/.claude/skills/toolkit');
    expect(useSkillUpdateWorkflow.getState().selectedCopyEntries.size).toBe(0);
  });

  it('keeps a stable dialog frame while the preview is loading', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'loadingPreview',
      context,
      skillNames: ['toolkit'],
      preview: {
        sources: [], blocked: [], redirectedDownloadHosts: [],
        skills: [{ targets: [automaticTarget()],
          skillName: 'toolkit',
          sourceDisplay: 'should-not-render-before-ready',
          refDisplay: 'HEAD',
          adapterTargets: [],
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          cleanCopyCount: 0,
          overwritePrivateEntries: [],
          blockingReasons: [],
          linkedTargets: [], fallbackForecasts: [],
        }],
      },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    const body = screen.getByTestId('update-plan-dialog-body');
    expect(body.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.queryByText('should-not-render-before-ready')).toBeNull();
    expect(screen.getByRole('button', { name: 'common.cancel' })).not.toBeNull();
  });

  it('focuses cancel and returns focus to the update entry when closed', async () => {
    useSkillUpdateWorkflow.setState({ phase: 'loadingPreview', context, skillNames: ['toolkit'] });
    const props = { context, skillNames: ['toolkit'], onOpenChange: vi.fn() };
    const { rerender } = render(<><button type="button">Update entry</button><UpdatePlanDialog open={false} {...props} /></>);
    const entry = screen.getByRole('button', { name: 'Update entry' });
    entry.focus();
    rerender(<><button type="button">Update entry</button><UpdatePlanDialog open {...props} /></>);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'common.cancel' }));
    rerender(<><button type="button">Update entry</button><UpdatePlanDialog open={false} {...props} /></>);
    await waitFor(() => expect(document.activeElement).toBe(entry));
  });

  it('ignores the overlay but allows Escape and the close button before execution', () => {
    const onOpenChange = vi.fn();
    useSkillUpdateWorkflow.setState({
      phase: 'loadingPreview', context, skillNames: ['toolkit'], batch: false,
    });

    render(
      <UpdatePlanDialog
        open
        context={context}
        skillNames={['toolkit']}
        onOpenChange={onOpenChange}
      />,
    );

    const overlay = document.querySelector('[data-slot="dialog-overlay"]');
    expect(overlay).not.toBeNull();
    fireEvent.pointerDown(overlay!);
    expect(onOpenChange).not.toHaveBeenCalled();

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onOpenChange).toHaveBeenCalledWith(false);

    onOpenChange.mockClear();
    fireEvent.click(screen.getByRole('button', { name: 'common.close' }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('uses workflow preview conflicts and confirmation instead of store legacy state', () => {
    const confirm = vi.fn();
    useSkillUpdateWorkflow.setState({
      phase: 'ready', context, skillNames: ['toolkit'], batch: false,
      preview: { sources: [], blocked: [], redirectedDownloadHosts: [], skills: [{ skillName: 'toolkit', sourceDisplay: 'github.com/owner/repo', refDisplay: 'HEAD', adapterTargets: [], targets: [], capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null }, cleanCopyCount: 0, overwritePrivateEntries: [{ entryId: 'private', displayPath: { environment: context.environment, nativePath: '/agents/private' }, readers: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex-private' }] }], blockingReasons: [], linkedTargets: [], fallbackForecasts: [] }] },
      selectedCopyEntries: new Set(), confirm,
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);
    expect((screen.getByRole('button', { name: 'skills.updatePlan.confirm' }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole('checkbox', { name: /Codex.*\/agents\/private/ }));
    fireEvent.click(screen.getByRole('button', { name: 'skills.updatePlan.confirm' }));

    expect(useSkillUpdateWorkflow.getState().selectedCopyEntries).toEqual(new Set(['private']));
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Codex')).toBeTruthy();
    expect(screen.queryByText('codex - codex-private')).toBeNull();
    expect(screen.getByText('/agents/private')).toBeTruthy();
  });

  it('renders Backend preview source ref and placement instead of a stale list plan', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'ready',
      context,
      skillNames: ['toolkit'],
      batch: false,
      preview: {
        sources: [], blocked: [], redirectedDownloadHosts: [],
        skills: [{
          skillName: 'toolkit',
          sourceDisplay: 'github.com/backend/repo',
          refDisplay: 'release',
          targets: [{ displayPath: { environment: context.environment, nativePath: '/actual/agent/skills/toolkit' }, readers: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex-private' }], restoring: false }],
          preservedTargets: [automaticTarget('/independent/skills/toolkit')],
          adapterTargets: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex-adapter' }],
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          cleanCopyCount: 0,
          overwritePrivateEntries: [],
          blockingReasons: [],
          linkedTargets: [], fallbackForecasts: [],
        }],
      },
    });

    render(<UpdatePlanDialog
      open
      context={context}
      skillNames={['toolkit']}
      agentDisplayNames={new Map([['codex', 'Codex']])}
      onOpenChange={vi.fn()}
    />);

    expect(screen.getByText('skills.updatePlan.source github.com/backend/repo · release')).toBeTruthy();
    expect(screen.getByText('/actual/agent/skills/toolkit')).toBeTruthy();
    expect(screen.getByText('/independent/skills/toolkit')).toBeTruthy();
    expect(screen.getAllByRole('checkbox').every((item) => (item as HTMLButtonElement).disabled)).toBe(true);
    expect(screen.queryByText('skills.refBadge:release')).toBeNull();
    expect(screen.queryByText('skills.updatePlan.adapterTargetsAction')).toBeNull();
    expect(screen.queryByText('stale/repo')).toBeNull();
    expect(screen.queryByText('legacy-agent')).toBeNull();
  });

  it('disambiguates duplicate owner names while keeping conflict decisions independent', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'ready', context, skillNames: ['toolkit'], batch: false,
      preview: {
        sources: [], blocked: [], redirectedDownloadHosts: [],
        skills: [{ targets: [automaticTarget()],
          skillName: 'toolkit',
          sourceDisplay: 'github.com/owner/repo',
          refDisplay: 'HEAD',
          adapterTargets: [],
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          cleanCopyCount: 0,
          overwritePrivateEntries: [
            { displayPath: { environment: context.environment, nativePath: "/agents/private-a" }, entryId: 'private-a', readers: [{ agentId: 'custom-a', displayName: 'Custom', logicalTargetId: 'target-a' }] },
            { displayPath: { environment: context.environment, nativePath: "/agents/private-b" }, entryId: 'private-b', readers: [{ agentId: 'custom-b', displayName: 'Custom', logicalTargetId: 'target-b' }] },
          ],
          blockingReasons: [],
          linkedTargets: [], fallbackForecasts: [],
        }],
      },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect(screen.getAllByText('Custom')).toHaveLength(2);
    expect(screen.queryByText('custom-a - target-a')).toBeNull();
    expect(screen.queryByText('custom-b - target-b')).toBeNull();
    fireEvent.click(screen.getByRole('checkbox', { name: /Custom.*\/agents\/private-a/ }));
    expect(useSkillUpdateWorkflow.getState().selectedCopyEntries).toEqual(new Set(['private-a']));
    fireEvent.click(screen.getByRole('checkbox', { name: /Custom.*\/agents\/private-b/ }));
    expect(useSkillUpdateWorkflow.getState().selectedCopyEntries).toEqual(new Set(['private-a', 'private-b']));
  });

  it('lists automatic update locations without making them selectable', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'ready', context, skillNames: ['toolkit'], batch: false,
      preview: { sources: [], blocked: [], redirectedDownloadHosts: [], skills: [{ targets: [automaticTarget(), automaticTarget('/agents/clean-1'), automaticTarget('/agents/clean-2')], skillName: 'toolkit', sourceDisplay: 'github.com/owner/repo', refDisplay: 'HEAD', adapterTargets: [], capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null }, cleanCopyCount: 2, overwritePrivateEntries: [{ displayPath: { environment: context.environment, nativePath: "/agents/private" }, entryId: 'private', readers: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex-private' }] }], blockingReasons: [], linkedTargets: [], fallbackForecasts: [] }] },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect(screen.queryByText('skills.updatePlan.cleanCopiesAction')).toBeNull();
    expect(within(screen.getByRole('list', { name: 'skills.updatePlan.updateLocations' })).getAllByRole('listitem')).toHaveLength(4);
    expect(screen.getAllByRole('checkbox').filter((item) => !(item as HTMLButtonElement).disabled)).toHaveLength(1);
  });

  it('keeps preview-error actions in the footer only', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'previewError', context, skillNames: ['toolkit'], batch: false,
    } as never);

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect(screen.getAllByRole('button', { name: 'common.cancel' })).toHaveLength(1);
    expect(screen.getAllByRole('button', { name: 'common.retry' })).toHaveLength(1);
  });

  it('renders results and retries through the workflow owner', () => {
    const retryFailed = vi.fn();
    useSkillUpdateWorkflow.setState({
      phase: 'result', context, skillNames: ['toolkit'], batch: false, retryFailed,
      result: { sources: [], skills: [{ skillIdentity: { context, skillName: 'toolkit' }, sourceResultId: '', mutation: null, coverage: { kind: 'notUpdated', error: { code: 'executionFailed', parameters: {}, field: null, severity: 'error', retryable: true, technicalDetails: null, environment: context.environment, context, unitId: 'toolkit', recoveryResourceId: null, displayPaths: [] } }, warnings: [], retryable: true }], outcome: 'failed' },
    } as never);

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'skills.updatePlan.retryFailed' }));
    expect(retryFailed).toHaveBeenCalledTimes(1);
  });

  it('reports an intentionally skipped copy without presenting it as a failed retry', () => {
    useSkillUpdateWorkflow.setState({ phase: 'result', context, skillNames: ['toolkit'],
      result: { sources: [], outcome: 'succeeded', skills: [{
        skillIdentity: { context, skillName: 'toolkit' }, sourceResultId: '',
        mutation: { unitId: 'update-toolkit', skillName: 'toolkit', source: null, target: context,
          status: 'succeeded', retryable: false, lockCommitted: true, actualMode: null, fallbackReason: null,
          agentTargets: [], warnings: [], error: null, recovery: null },
        coverage: { kind: 'updatedWithSkippedCopies' }, warnings: ['skippedCopy'], retryable: false,
        skippedCopyPaths: [{ environment: context.environment, nativePath: '/cursor/toolkit' }],
      }] },
    });
    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);
    expect(screen.getByText('skills.updatePlan.updatedWithSkippedCopies')).toBeTruthy();
    expect(screen.getByText('/cursor/toolkit')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'skills.updatePlan.retryFailed' })).toBeNull();
  });

  it('keeps cancel available but blocks confirmation during another mutation', () => {
    useSkillUpdateWorkflow.setState({ phase: 'ready', context, skillNames: ['toolkit'] });
    useMutationStore.setState({ activeMutation: { id: 'other-mutation' } as never });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect((screen.getByRole('button', { name: 'skills.updatePlan.confirm' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'common.cancel' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('presents a cancelled result without offering a retry', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'result', context, skillNames: ['toolkit'], batch: false,
      result: { sources: [], skills: [{ skillIdentity: { context, skillName: 'toolkit' }, sourceResultId: '', mutation: null, coverage: { kind: 'notUpdated', error: { code: 'mutationCancelled', parameters: {}, field: null, severity: 'error', retryable: false, technicalDetails: null, environment: context.environment, context, unitId: 'toolkit', recoveryResourceId: null, displayPaths: [] } }, warnings: [], retryable: false }], outcome: 'cancelled' },
    } as never);

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect(screen.getByText('skills.updatePlan.resultOutcome.cancelled')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'skills.updatePlan.retryFailed' })).toBeNull();
  });

  it('shows a shared source error once instead of repeating it on every Skill row', () => {
    const error = { code: 'executionFailed', parameters: {}, field: null, severity: 'error', retryable: true, technicalDetails: null, environment: context.environment, context, unitId: null, recoveryResourceId: null, displayPaths: [] } as const;
    useSkillUpdateWorkflow.setState({
      phase: 'result', context, skillNames: ['toolkit', 'reviewer'], batch: true,
      result: { sources: [{ id: 'source-1', source: 'owner/repo', status: 'failed', error }], skills: ['toolkit', 'reviewer'].map((skillName) => ({ skillIdentity: { context, skillName }, sourceResultId: 'source-1', mutation: null, coverage: { kind: 'notUpdated' as const, error }, warnings: [], retryable: true })), outcome: 'failed' },
    } as never);

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit', 'reviewer']} onOpenChange={vi.fn()} />);

    expect(screen.getAllByText('mutation.result.errors.executionFailed')).toHaveLength(1);
  });

  it('allows direct reinstall when preview permits it even though the display plan has no update row', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'ready', context, skillNames: ['toolkit'], batch: false,
      preview: { sources: [], blocked: [], redirectedDownloadHosts: [], skills: [{ targets: [automaticTarget()], skillName: 'toolkit', sourceDisplay: 'github.com/owner/repo', refDisplay: 'HEAD', adapterTargets: [], capability: { canRunUpdate: true, canCheckForUpdates: false, reason: 'missingRemoteHash' }, cleanCopyCount: 0, overwritePrivateEntries: [], blockingReasons: [], linkedTargets: [], fallbackForecasts: [] }] },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);
    expect((screen.getByRole('button', { name: 'skills.updatePlan.confirm' }) as HTMLButtonElement).disabled).toBe(false);
    expect(screen.getByRole('heading', { name: 'skills.updatePlan.globalTitle' })).toBeTruthy();
  });

  it('requests cancellation and remains open when its active update can still be cancelled', () => {
    const onOpenChange = vi.fn();
    const cancelActiveMutation = vi.fn().mockResolvedValue(true);
    const activeMutation: ActiveMutation = {
      id: 'update-1', kind: 'update', target, phase: 'acquiring', progress: null, cancelable: true,
    };
    useSkillUpdateWorkflow.setState({ phase: 'executing', context, skillNames: ['toolkit'] });
    useMutationStore.setState({ activeMutation, cancelActiveMutation });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={onOpenChange} />);
    fireEvent.click(screen.getByRole('button', { name: 'skills.updatePlan.stop' }));

    expect(cancelActiveMutation).toHaveBeenCalledTimes(1);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it('keeps implicit dismissal separate from explicitly stopping an active update', () => {
    const onOpenChange = vi.fn();
    const cancelActiveMutation = vi.fn().mockResolvedValue(true);
    const activeMutation: ActiveMutation = {
      id: 'update-1', kind: 'update', target, phase: 'acquiring', progress: null, cancelable: true,
    };
    useSkillUpdateWorkflow.setState({ phase: 'executing', context, skillNames: ['toolkit'] });
    useMutationStore.setState({ activeMutation, cancelActiveMutation });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={onOpenChange} />);
    fireEvent.keyDown(document, { key: 'Escape' });

    expect(cancelActiveMutation).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'skills.updatePlan.stop' }));
    expect(cancelActiveMutation).toHaveBeenCalledTimes(1);
  });

  it('blocks close while confirmation is waiting for mutation admission', () => {
    const onOpenChange = vi.fn();
    useSkillUpdateWorkflow.setState({
      phase: 'executing',
      context,
      skillNames: ['toolkit'],
      confirming: true,
      preview: {
        sources: [], blocked: [], redirectedDownloadHosts: [],
        skills: [{ targets: [automaticTarget()],
          skillName: 'toolkit',
          sourceDisplay: 'github.com/owner/repo',
          refDisplay: 'HEAD',
          adapterTargets: [],
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          cleanCopyCount: 0,
          overwritePrivateEntries: [],
          blockingReasons: [],
          linkedTargets: [], fallbackForecasts: [],
        }],
      },
    });
    useMutationStore.setState({ activeMutation: null });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={onOpenChange} />);

    expect(screen.queryByRole('button', { name: 'common.cancel' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Close' })).toBeNull();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it('removes closing controls while its active update is irreversible', () => {
    const onOpenChange = vi.fn();
    const activeMutation: ActiveMutation = {
      id: 'update-1', kind: 'update', target, phase: 'committing', progress: null, cancelable: false,
    };
    useSkillUpdateWorkflow.setState({ phase: 'executing', context, skillNames: ['toolkit'] });
    useMutationStore.setState({ activeMutation });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={onOpenChange} />);

    expect(screen.queryByRole('button', { name: 'common.cancel' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Close' })).toBeNull();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it.each(['acquiring', 'validating', 'committing'] as const)(
    'announces the %s phase with Backend progress',
    (phase) => {
      const activeMutation: ActiveMutation = {
        id: 'update-1', kind: 'update', target,
        phase,
        progress: { subject: '/private/path', current: 2, total: 5 }, cancelable: true,
      };
      useSkillUpdateWorkflow.setState({ phase: 'executing', context, skillNames: ['toolkit'] });
      useMutationStore.setState({ activeMutation });

      render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

      expect(screen.getByRole('status').getAttribute('aria-live')).toBe('polite');
      expect(screen.getByText(`mutation.phase.${phase}`)).toBeTruthy();
      expect(screen.queryByText('skills.updatePlan.progress')).toBeNull();
      expect(screen.queryByText('/private/path')).toBeNull();
    },
  );

  it('shows count progress while a batch update is executing', () => {
    const activeMutation: ActiveMutation = {
      id: 'update-1', kind: 'update', target, phase: 'committing',
      progress: { subject: 'reviewer', current: 2, total: 5 }, cancelable: true,
    };
    useSkillUpdateWorkflow.setState({
      phase: 'executing', context, skillNames: ['toolkit', 'reviewer'], batch: true,
    });
    useMutationStore.setState({ activeMutation });

    render(
      <UpdatePlanDialog
        open
        context={context}
        skillNames={['toolkit', 'reviewer']}
        onOpenChange={vi.fn()}
      />,
    );

    expect(screen.getByText('skills.updatePlan.progress')).toBeTruthy();
    expect(screen.getByRole('progressbar')).toBeTruthy();
  });

  it('shows a command-level execution error once without offering retry', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'result', context, skillNames: ['toolkit'], batch: false,
      result: null, executionError: { kind: 'custom', data: { message: 'command failed' } },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect(screen.getByText('command failed')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'skills.updatePlan.retryFailed' })).toBeNull();
  });
});
