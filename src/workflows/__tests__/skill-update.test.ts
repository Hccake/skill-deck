import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppError, ContextRef, UpdatePreview, UpdateResponse } from '@/bindings';
import { contextKey } from '@/lib/context';

const mocks = vi.hoisted(() => ({
  previewUpdate: vi.fn<() => Promise<UpdatePreview>>(),
  updateSkill: vi.fn<() => Promise<UpdateResponse>>(),
  updateSkillsBatch: vi.fn<() => Promise<UpdateResponse>>(),
  applyUpdateResult: vi.fn(),
  snapshots: {} as Record<string, unknown>,
}));

vi.mock('@/hooks/useTauriApi', () => mocks);
vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: { getState: () => ({ applyUpdateResult: mocks.applyUpdateResult, snapshots: mocks.snapshots }) },
}));

import { useSkillUpdateWorkflow } from '../skill-update';

const context: ContextRef = { environment: { kind: 'host' }, scope: { scope: 'global' } };
const preview = (name = 'demo'): UpdatePreview => ({
  token: { generation: 'preview-1', registryRevision: 'registry-1', environmentRevision: 'environment-1', contextRevision: 'context-1' },
  skills: [{
    skillName: name,
    sourceDisplay: 'github.com/backend/repo',
    refDisplay: 'release',
    placementAgentIds: ['codex'],
    capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
    cleanCopyCount: 0,
    overwritePrivateEntries: [],
    blockingReasons: [],
    fallbackForecasts: [],
  }],
});

describe('skill update workflow', () => {
  beforeEach(() => {
    useSkillUpdateWorkflow.getState().reset();
    mocks.previewUpdate.mockReset();
    mocks.updateSkill.mockReset();
    mocks.updateSkillsBatch.mockReset();
    mocks.applyUpdateResult.mockReset();
    mocks.snapshots = {};
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
    mocks.previewUpdate.mockResolvedValue(preview('toolkit'));

    await useSkillUpdateWorkflow.getState().open(context, ['toolkit'], false);

    expect(useSkillUpdateWorkflow.getState().preview).toEqual(preview('toolkit'));
    expect(useSkillUpdateWorkflow.getState()).not.toHaveProperty('plan');
  });

  it('opens synchronously, freezes its request, and cancellation never executes', async () => {
    let resolvePreview!: (value: UpdatePreview) => void;
    mocks.previewUpdate.mockReturnValue(new Promise((resolve) => { resolvePreview = resolve; }));
    const pending = useSkillUpdateWorkflow.getState().open(context, ['demo']);
    expect(useSkillUpdateWorkflow.getState()).toMatchObject({ phase: 'loadingPreview', context, skillNames: ['demo'] });
    useSkillUpdateWorkflow.getState().close();
    await useSkillUpdateWorkflow.getState().confirm();
    expect(mocks.updateSkill).not.toHaveBeenCalled();
    resolvePreview(preview());
    await pending;
    expect(useSkillUpdateWorkflow.getState().phase).toBe('closed');
  });

  it('does not let an old preview overwrite the newer operation', async () => {
    let first!: (value: UpdatePreview) => void;
    mocks.previewUpdate.mockImplementationOnce(() => new Promise((resolve) => { first = resolve; }));
    mocks.previewUpdate.mockResolvedValueOnce(preview('newer'));
    const initial = useSkillUpdateWorkflow.getState().open(context, ['older']);
    await useSkillUpdateWorkflow.getState().open(context, ['newer']);
    first(preview('older'));
    await initial;
    expect(useSkillUpdateWorkflow.getState().preview?.skills[0]?.skillName).toBe('newer');
  });

  it('preserves conflicts unless explicitly selected', async () => {
    mocks.previewUpdate.mockResolvedValue({ ...preview(), skills: [{ ...preview().skills[0]!, overwritePrivateEntries: [{ entryId: 'private-entry', owners: [] }] }] });
    mocks.updateSkill.mockResolvedValue({ sources: [], skills: [], outcome: 'succeeded' });
    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    await useSkillUpdateWorkflow.getState().confirm();
    expect(mocks.updateSkill).toHaveBeenCalledWith(expect.objectContaining({ overwritePrivateEntries: [] }), expect.anything());
  });

  it('applies the completed result through the snapshot facade without storing it there', async () => {
    const result: UpdateResponse = { sources: [], skills: [], outcome: 'succeeded' };
    mocks.previewUpdate.mockResolvedValue(preview());
    mocks.updateSkill.mockResolvedValue(result);

    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    await useSkillUpdateWorkflow.getState().confirm();

    expect(mocks.applyUpdateResult).toHaveBeenCalledWith(context, result);
    expect(useSkillUpdateWorkflow.getState()).toMatchObject({ phase: 'result', result });
  });

  it('sends only one execution request while confirmation is pending', async () => {
    let resolveUpdate!: (value: UpdateResponse) => void;
    mocks.previewUpdate.mockResolvedValue(preview());
    mocks.updateSkill.mockReturnValue(new Promise((resolve) => { resolveUpdate = resolve; }));
    await useSkillUpdateWorkflow.getState().open(context, ['demo']);

    const first = useSkillUpdateWorkflow.getState().confirm();
    const second = useSkillUpdateWorkflow.getState().confirm();

    expect(mocks.updateSkill).toHaveBeenCalledTimes(1);
    resolveUpdate({ sources: [], skills: [], outcome: 'succeeded' });
    await Promise.all([first, second]);
  });

  it('does not let a completed operation overwrite a newer generation after refresh', async () => {
    let resolveRefresh!: () => void;
    mocks.previewUpdate.mockResolvedValueOnce(preview('older'));
    mocks.updateSkill.mockResolvedValue({ sources: [], skills: [], outcome: 'succeeded' });
    mocks.applyUpdateResult.mockReturnValue(new Promise<void>((resolve) => { resolveRefresh = resolve; }));

    await useSkillUpdateWorkflow.getState().open(context, ['older']);
    const confirming = useSkillUpdateWorkflow.getState().confirm();
    await Promise.resolve();

    mocks.previewUpdate.mockResolvedValueOnce(preview('newer'));
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
    mocks.previewUpdate.mockResolvedValue(preview());
    mocks.updateSkill.mockRejectedValue(commandError);

    await useSkillUpdateWorkflow.getState().open(context, ['demo']);
    await useSkillUpdateWorkflow.getState().confirm();

    expect(mocks.applyUpdateResult).not.toHaveBeenCalled();
    expect(useSkillUpdateWorkflow.getState()).toMatchObject({
      phase: 'result',
      result: null,
      executionError: commandError,
    });
  });
});
