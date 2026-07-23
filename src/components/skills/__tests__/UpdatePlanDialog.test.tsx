/* @vitest-environment jsdom */
import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { UpdatePlanDialog } from '../UpdatePlanDialog';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useMutationStore } from '@/stores/mutation';
import type { ActiveMutation, ContextRef } from '@/bindings';
import { contextKey } from '@/lib/context';

vi.mock('react-i18next', () => ({ useTranslation: () => ({
  t: (key: string, options?: { count?: number; ref?: string }) => {
    if (key === 'skills.updatePlan.readyTitle') return `${key}:${options?.count}`;
    if (key === 'skills.refBadge') return `${key}:${options?.ref}`;
    return key;
  },
}) }));

const context: ContextRef = { environment: { kind: 'host' }, scope: { scope: 'global' } };

describe('UpdatePlanDialog', () => {
  beforeEach(() => {
    useSkillUpdateWorkflow.getState().reset();
    useSkillsDataStore.setState({ snapshots: {
      [contextKey(context)]: {
        skills: [{ name: 'toolkit', description: '', path: '/skills/toolkit', canonicalPath: '/canonical/toolkit', scope: 'global', agents: [], source: 'owner/repo', hasUpdate: true, canRunUpdate: true, canCheckForUpdates: true, updateStatus: 'updateAvailable', updateReason: null }],
        agents: [], pathExists: true, loading: false, error: null, requestId: 1,
      },
    } });
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('keeps a stable dialog frame while the preview is loading', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'loadingPreview',
      context,
      skillNames: ['toolkit'],
      preview: {
        token: { generation: 'stale-preview', registryRevision: 'registry-1', environmentRevision: 'environment-1', contextRevision: 'context-1' },
        skills: [{
          skillName: 'toolkit',
          sourceDisplay: 'should-not-render-before-ready',
          refDisplay: 'HEAD',
          adapterTargets: [],
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          cleanCopyCount: 0,
          overwritePrivateEntries: [],
          blockingReasons: [],
          fallbackForecasts: [],
        }],
      },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    const dialog = screen.getByRole('dialog');
    const body = screen.getByTestId('update-plan-dialog-body');
    expect(dialog.className).toContain('max-h-[min(32rem,calc(100dvh-2rem))]');
    expect(dialog.className).toContain('grid-rows-[auto_minmax(0,1fr)_auto]');
    expect(body.className).toContain('min-h-0');
    expect(body.className).toContain('overflow-y-auto');
    expect(body.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThan(0);
    expect(screen.queryByText('should-not-render-before-ready')).toBeNull();
    expect(screen.getByRole('button', { name: 'common.cancel' })).not.toBeNull();
  });

  it('keeps the larger stable frame for batch updates', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'loadingPreview',
      context,
      skillNames: ['toolkit', 'reviewer'],
      batch: true,
    });

    render(
      <UpdatePlanDialog
        open
        context={context}
        skillNames={['toolkit', 'reviewer']}
        onOpenChange={vi.fn()}
      />,
    );

    expect(screen.getByRole('dialog').className)
      .toContain('h-[min(40rem,calc(100dvh-2rem))]');
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
      preview: { token: { generation: 'preview-1', registryRevision: 'registry-1', environmentRevision: 'environment-1', contextRevision: 'context-1' }, skills: [{ skillName: 'toolkit', sourceDisplay: 'github.com/owner/repo', refDisplay: 'HEAD', adapterTargets: [], capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null }, cleanCopyCount: 0, overwritePrivateEntries: [{ entryId: 'private', owners: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex-private' }] }], blockingReasons: [], fallbackForecasts: [] }] },
      conflictDecisions: new Set(), confirm,
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'skills.updatePlan.overwritePrivateEntry' }));
    fireEvent.click(screen.getByRole('button', { name: 'skills.updatePlan.confirm' }));

    expect(useSkillUpdateWorkflow.getState().conflictDecisions).toEqual(new Set(['private']));
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Codex')).toBeTruthy();
    expect(screen.queryByText('codex - codex-private')).toBeNull();
    expect(screen.queryByText('/agents/private')).toBeNull();
  });

  it('renders Backend preview source ref and placement instead of a stale list plan', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'ready',
      context,
      skillNames: ['toolkit'],
      batch: false,
      preview: {
        token: { generation: 'preview-1', registryRevision: 'registry-1', environmentRevision: 'environment-1', contextRevision: 'context-1' },
        skills: [{
          skillName: 'toolkit',
          sourceDisplay: 'github.com/backend/repo',
          refDisplay: 'release',
          adapterTargets: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex-adapter' }],
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          cleanCopyCount: 0,
          overwritePrivateEntries: [],
          blockingReasons: [],
          fallbackForecasts: [],
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

    expect(screen.getByText('github.com/backend/repo')).toBeTruthy();
    expect(screen.getByText('skills.refBadge:release')).toBeTruthy();
    expect(screen.getByText('skills.updatePlan.adapterTargetsAction')).toBeTruthy();
    expect(screen.queryByText('stale/repo')).toBeNull();
    expect(screen.queryByText('legacy-agent')).toBeNull();
  });

  it('disambiguates duplicate owner names while keeping conflict decisions independent', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'ready', context, skillNames: ['toolkit'], batch: false,
      preview: {
        token: { generation: 'preview-1', registryRevision: 'registry-1', environmentRevision: 'environment-1', contextRevision: 'context-1' },
        skills: [{
          skillName: 'toolkit',
          sourceDisplay: 'github.com/owner/repo',
          refDisplay: 'HEAD',
          adapterTargets: [],
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          cleanCopyCount: 0,
          overwritePrivateEntries: [
            { entryId: 'private-a', owners: [{ agentId: 'custom-a', displayName: 'Custom', logicalTargetId: 'target-a' }] },
            { entryId: 'private-b', owners: [{ agentId: 'custom-b', displayName: 'Custom', logicalTargetId: 'target-b' }] },
          ],
          blockingReasons: [],
          fallbackForecasts: [],
        }],
      },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect(screen.getAllByText('Custom')).toHaveLength(2);
    expect(screen.queryByText('custom-a - target-a')).toBeNull();
    expect(screen.queryByText('custom-b - target-b')).toBeNull();
    const checkboxes = screen.getAllByRole('checkbox', { name: 'skills.updatePlan.overwritePrivateEntry' });
    fireEvent.click(checkboxes[0]!);
    expect(useSkillUpdateWorkflow.getState().conflictDecisions).toEqual(new Set(['private-a']));
    fireEvent.click(checkboxes[1]!);
    expect(useSkillUpdateWorkflow.getState().conflictDecisions).toEqual(new Set(['private-a', 'private-b']));
  });

  it('shows clean-copy totals without making clean copies selectable', () => {
    useSkillUpdateWorkflow.setState({
      phase: 'ready', context, skillNames: ['toolkit'], batch: false,
      preview: { token: { generation: 'preview-1', registryRevision: 'registry-1', environmentRevision: 'environment-1', contextRevision: 'context-1' }, skills: [{ skillName: 'toolkit', sourceDisplay: 'github.com/owner/repo', refDisplay: 'HEAD', adapterTargets: [], capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null }, cleanCopyCount: 2, overwritePrivateEntries: [{ entryId: 'private', owners: [{ agentId: 'codex', displayName: 'Codex', logicalTargetId: 'codex-private' }] }], blockingReasons: [], fallbackForecasts: [] }] },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);

    expect(screen.getByText('skills.updatePlan.cleanCopiesAction')).toBeTruthy();
    expect(screen.getAllByRole('checkbox', { name: 'skills.updatePlan.overwritePrivateEntry' })).toHaveLength(1);
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
      preview: { token: { generation: 'preview-1', registryRevision: 'registry-1', environmentRevision: 'environment-1', contextRevision: 'context-1' }, skills: [{ skillName: 'toolkit', sourceDisplay: 'github.com/owner/repo', refDisplay: 'HEAD', adapterTargets: [], capability: { canRunUpdate: true, canCheckForUpdates: false, reason: 'missingRemoteHash' }, cleanCopyCount: 0, overwritePrivateEntries: [], blockingReasons: [], fallbackForecasts: [] }] },
    });

    render(<UpdatePlanDialog open context={context} skillNames={['toolkit']} onOpenChange={vi.fn()} />);
    expect((screen.getByRole('button', { name: 'skills.updatePlan.confirm' }) as HTMLButtonElement).disabled).toBe(false);
    expect(screen.getByRole('heading', { name: 'skills.updatePlan.singleTitle' })).toBeTruthy();
  });

  it('requests cancellation and remains open when its active update can still be cancelled', () => {
    const onOpenChange = vi.fn();
    const cancelActiveMutation = vi.fn().mockResolvedValue(true);
    const activeMutation: ActiveMutation = {
      id: 'update-1', kind: 'update', context, phase: 'acquiring', progress: null, cancelable: true,
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
      id: 'update-1', kind: 'update', context, phase: 'acquiring', progress: null, cancelable: true,
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
        token: { generation: 'preview-1', registryRevision: 'registry-1', environmentRevision: 'environment-1', contextRevision: 'context-1' },
        skills: [{
          skillName: 'toolkit',
          sourceDisplay: 'github.com/owner/repo',
          refDisplay: 'HEAD',
          adapterTargets: [],
          capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
          cleanCopyCount: 0,
          overwritePrivateEntries: [],
          blockingReasons: [],
          fallbackForecasts: [],
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
      id: 'update-1', kind: 'update', context, phase: 'committing', progress: null, cancelable: false,
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
        id: 'update-1', kind: 'update', context,
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
      id: 'update-1', kind: 'update', context, phase: 'committing',
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
