import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppError, SkillLocationRef, PreparedUpdatePreview, UpdateResponse } from '@/bindings';
import { contextKey } from '@/lib/context';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

const mocks = vi.hoisted(() => ({
  prepareUpdate: vi.fn<() => Promise<PreparedUpdatePreview>>(),
  cancelUpdatePreparation: vi.fn(async () => {}),
  executeUpdate: vi.fn<() => Promise<UpdateResponse>>(),
  applyUpdateResult: vi.fn(),
  snapshots: {} as Record<string, unknown>,
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => mocks);
vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: { getState: () => ({ applyUpdateResult: mocks.applyUpdateResult, snapshots: mocks.snapshots }) },
}));

import { useSkillUpdateWorkflow } from '../skill-update';

const context: SkillLocationRef = { environment: { kind: 'native' }, scope: { scope: 'global' } };
const preview = (name = 'demo'): PreparedUpdatePreview => ({
  sources: [], blocked: [], redirectedDownloadHosts: [],
  skills: [{
    skillName: name,
    sourceDisplay: 'github.com/backend/repo',
    refDisplay: 'release',
    adapterTargets: [],
    targets: [{ displayPath: { environment: context.environment, nativePath: `/agents/${name}` }, readers: [], restoring: false }],
    capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
    cleanCopyCount: 0,
    overwritePrivateEntries: [],
    blockingReasons: [],
    linkedTargets: [], fallbackForecasts: [],
  }],
});

describe('skill update workflow', () => {
  beforeEach(() => {
    useSkillUpdateWorkflow.getState().reset();
    mocks.prepareUpdate.mockReset();
    mocks.executeUpdate.mockReset();
    mocks.applyUpdateResult.mockReset();
    mocks.snapshots = {};
    mocks.getInstallWizardSession.mockResolvedValue({ revision: 1, active: true });
    useInstallWizardSessionStore.setState({
      revision: 0, active: false, loading: false, hasConfirmedSnapshot: false,
      syncError: null, monitorRetryRevision: 0, snapshotVersion: 0,
    });
  });

  it('keeps Backend preview as the only display authority', async () => {
    mocks.snapshots = {
      [contextKey(context)]: {
        skills: [{
          name: 'toolkit', description: '', path: '/skills/toolkit', canonicalPath: '/canonical/toolkit',
          scope: 'global', agents: ['legacy-agent'], source: 'stale/repo', hasUpdate: false,
          canRunUpdate: true, canCheckForUpdates: false, updateStatus: 'cannotCheck',
          updateReason: 'missingRemoteHash',
        }],
      },
    };
    mocks.prepareUpdate.mockResolvedValue(preview('toolkit'));

    await useSkillUpdateWorkflow.getState().open(context, ['toolkit'], false);

    expect(useSkillUpdateWorkflow.getState().preview).toEqual(preview('toolkit'));
  });

  it('opens synchronously, freezes its request, and cancellation never executes', async () => {
    let resolvePreview!: (value: PreparedUpdatePreview) => void;
    mocks.prepareUpdate.mockReturnValue(new Promise((resolve) => { resolvePreview = resolve; }));
    const pending = useSkillUpdateWorkflow.getState().open(context, ['demo']);
    expect(useSkillUpdateWorkflow.getState()).toMatchObject({ phase: 'loadingPreview', context, skillNames: ['demo'] });
    useSkillUpdateWorkflow.getState().close();
    await useSkillUpdateWorkflow.getState().confirm();
    expect(mocks.executeUpdate).not.toHaveBeenCalled();
    resolvePreview(preview());
    await pending;
    expect(useSkillUpdateWorkflow.getState().phase).toBe('closed');
  });

  it('does not let an old preview overwrite the newer operation', async () => {
    let first!: (value: PreparedUpdatePreview) => void;
    mocks.prepareUpdate.mockImplementationOnce(() => new Promise((resolve) => { first = resolve; }));
    mocks.prepareUpdate.mockResolvedValueOnce(preview('newer'));
    const initial = useSkillUpdateWorkflow.getState().open(context, ['older']);
    await useSkillUpdateWorkflow.getState().open(context, ['newer']);
    first(preview('older'));
    await initial;
    expect(useSkillUpdateWorkflow.getState().preview?.skills[0]?.skillName).toBe('newer');
  });

  it('preserves conflicts unless explicitly selected', async () => {
    mocks.prepareUpdate.mockResolvedValue({ ...preview(), skills: [{ ...preview().skills[0]!, overwritePrivateEntries: [{ entryId: 'private-entry', readers: [], displayPath: { environment: context.environment, nativePath: '/agents/private-entry' } }] }] });
    mocks.executeUpdate.mockResolvedValue({ sources: [], skills: [], outcome: 'succeeded' });
    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    await useSkillUpdateWorkflow.getState().confirm();
    expect(mocks.executeUpdate).toHaveBeenCalledWith(
      expect.any(String), [],
    );
  });

  it('selects matching copies by default and stops confirmation when every optional copy is unchecked', async () => {
    const value = preview();
    value.skills[0].targets = [{ ...value.skills[0].targets[0], selectableEntryId: 'clean-copy' }];
    value.skills[0].overwritePrivateEntries = [{ entryId: 'different-copy', readers: [], displayPath: { environment: context.environment, nativePath: '/copy/different' } }];
    mocks.prepareUpdate.mockResolvedValue(value);
    mocks.executeUpdate.mockResolvedValue({ sources: [], skills: [], outcome: 'succeeded' });
    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    expect(useSkillUpdateWorkflow.getState().selectedCopyEntries).toEqual(new Set(['clean-copy']));
    useSkillUpdateWorkflow.getState().setCopySelected('clean-copy', false);
    await useSkillUpdateWorkflow.getState().confirm();
    expect(mocks.executeUpdate).not.toHaveBeenCalled();
    useSkillUpdateWorkflow.getState().setCopySelected('different-copy', true);
    await useSkillUpdateWorkflow.getState().confirm();
    expect(mocks.executeUpdate).toHaveBeenCalledWith(expect.any(String), ['different-copy']);
  });

  it('shows all download hosts before the single execution confirmation', async () => {
    mocks.prepareUpdate.mockResolvedValue({ ...preview(), redirectedDownloadHosts: ['cdn.example.com', 'assets.example.com'] });
    mocks.executeUpdate.mockResolvedValue({ sources: [], skills: [], outcome: 'succeeded' });
    await useSkillUpdateWorkflow.getState().open(context, ['demo']);

    expect(useSkillUpdateWorkflow.getState()).toMatchObject({
      phase: 'ready',
      preview: { redirectedDownloadHosts: ['cdn.example.com', 'assets.example.com'] },
    });
    expect(mocks.executeUpdate).not.toHaveBeenCalled();
    await useSkillUpdateWorkflow.getState().confirm();
    expect(mocks.executeUpdate).toHaveBeenLastCalledWith(
      expect.any(String), [],
    );
    expect(mocks.executeUpdate).toHaveBeenCalledOnce();
    expect(useSkillUpdateWorkflow.getState().phase).toBe('result');
  });

  it('applies the completed result through the snapshot facade without storing it there', async () => {
    const result: UpdateResponse = { sources: [], skills: [], outcome: 'succeeded' };
    mocks.prepareUpdate.mockResolvedValue(preview());
    mocks.executeUpdate.mockResolvedValue(result);

    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    await useSkillUpdateWorkflow.getState().confirm();

    expect(mocks.applyUpdateResult).toHaveBeenCalledWith(context, result);
    expect(useSkillUpdateWorkflow.getState()).toMatchObject({ phase: 'result', result });
  });

  it('sends only one execution request while confirmation is pending', async () => {
    let resolveUpdate!: (value: UpdateResponse) => void;
    mocks.prepareUpdate.mockResolvedValue(preview());
    mocks.executeUpdate.mockReturnValue(new Promise((resolve) => { resolveUpdate = resolve; }));
    await useSkillUpdateWorkflow.getState().open(context, ['demo']);

    const first = useSkillUpdateWorkflow.getState().confirm();
    const second = useSkillUpdateWorkflow.getState().confirm();

    expect(mocks.executeUpdate).toHaveBeenCalledTimes(1);
    resolveUpdate({ sources: [], skills: [], outcome: 'succeeded' });
    await Promise.all([first, second]);
  });

  it('does not let a completed operation overwrite a newer generation after refresh', async () => {
    let resolveRefresh!: () => void;
    mocks.prepareUpdate.mockResolvedValueOnce(preview('older'));
    mocks.executeUpdate.mockResolvedValue({ sources: [], skills: [], outcome: 'succeeded' });
    mocks.applyUpdateResult.mockReturnValue(new Promise<void>((resolve) => { resolveRefresh = resolve; }));

    await useSkillUpdateWorkflow.getState().open(context, ['older']);
    const confirming = useSkillUpdateWorkflow.getState().confirm();
    await Promise.resolve();

    mocks.prepareUpdate.mockResolvedValueOnce(preview('newer'));
    await useSkillUpdateWorkflow.getState().open(context, ['newer']);
    resolveRefresh();
    await confirming;

    expect(useSkillUpdateWorkflow.getState()).toMatchObject({
      phase: 'ready',
      skillNames: ['newer'],
      preview: preview('newer'),
      result: null,
    });
  });

  it('preserves a command AppError without inventing retryable Skill results', async () => {
    const commandError: AppError = { kind: 'mutationBusy' };
    mocks.prepareUpdate.mockResolvedValue(preview());
    mocks.executeUpdate.mockRejectedValue(commandError);

    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    await useSkillUpdateWorkflow.getState().confirm();

    expect(mocks.applyUpdateResult).not.toHaveBeenCalled();
    expect(useSkillUpdateWorkflow.getState()).toMatchObject({
      phase: 'result',
      result: null,
      executionError: commandError,
    });
  });

  it('returns to the ready phase when installation wins update admission', async () => {
    mocks.prepareUpdate.mockResolvedValue(preview());
    mocks.executeUpdate.mockRejectedValue({ kind: 'installWizardActive' });

    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    await useSkillUpdateWorkflow.getState().confirm();

    expect(useSkillUpdateWorkflow.getState()).toMatchObject({
      phase: 'ready',
      result: null,
      executionError: null,
      confirming: false,
    });
    expect(mocks.applyUpdateResult).not.toHaveBeenCalled();
  });

  it('prepares expired content again without repeating execution automatically', async () => {
    mocks.prepareUpdate.mockResolvedValue(preview());
    mocks.executeUpdate.mockRejectedValue({ kind: 'stalePayload' });
    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    const operationId = useSkillUpdateWorkflow.getState().operationId;
    await useSkillUpdateWorkflow.getState().confirm();
    await useSkillUpdateWorkflow.getState().retryFailed();
    expect(useSkillUpdateWorkflow.getState().phase).toBe('ready');
    expect(useSkillUpdateWorkflow.getState().operationId).not.toBe(operationId);
    expect(mocks.prepareUpdate).toHaveBeenCalledTimes(2);
    expect(mocks.executeUpdate).toHaveBeenCalledOnce();
  });
});
