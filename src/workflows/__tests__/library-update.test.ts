import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  PreparedLibraryUpdatePreview,
  SkillUpdateInfo,
  UpdateCheckResponse,
} from '@/bindings';

const mocks = vi.hoisted(() => ({
  checkLibrarySkillUpdates: vi.fn(),
  prepareLibrarySkillUpdates: vi.fn(),
  cancelUpdatePreparation: vi.fn(async () => {}),
  updateLibrarySkills: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => mocks);

import { useLibraryUpdateWorkflow } from '../library-update';

const environment = { kind: 'native' as const };
const updateInfo = (status: SkillUpdateInfo['status']): SkillUpdateInfo => ({
  name: 'demo',
  source: 'owner/repo',
  hasUpdate: status === 'updateAvailable',
  status,
  reason: status === 'cannotCheck' ? 'upstreamUnavailable' : null,
  freshness: status === 'cannotCheck' ? 'unavailable' : 'fresh',
  capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
  gitRef: null,
  sourceUrl: 'https://github.com/owner/repo',
  skillPath: 'skills/demo',
  comparisonFingerprint: 'comparison-demo',
});
const checkResponse = (skill: SkillUpdateInfo, outcome: UpdateCheckResponse['outcome']): UpdateCheckResponse => ({
  outcome,
  sources: [],
  skills: [skill],
});
const preview: PreparedLibraryUpdatePreview = {
  blocked: [], redirectedDownloadHosts: [],
  skillNames: ['demo'],
};

const completed = () => ({
  sources: [], results: [{ skillName: 'demo', status: 'succeeded', sourceResultId: 'source-1', contentCommit: 'succeeded', catalogCommit: 'succeeded', error: null }],
  outcome: 'succeeded', library: null, membership: { scopes: [], cleanup: [], snapshotError: null },
});

describe('library update workflow', () => {
  beforeEach(() => {
    useLibraryUpdateWorkflow.getState().reset();
    mocks.checkLibrarySkillUpdates.mockReset();
    mocks.prepareLibrarySkillUpdates.mockReset();
    mocks.updateLibrarySkills.mockReset();
    mocks.cancelUpdatePreparation.mockClear();
    useLibraryUpdateWorkflow.getState().activate(environment, 'library-1');
  });

  it('keeps the confirmed comparison and the latest error on the same baseline', async () => {
    const error = { kind: 'io' as const, data: { message: 'offline' } };
    mocks.checkLibrarySkillUpdates
      .mockResolvedValueOnce(checkResponse(updateInfo('updateAvailable'), 'completed'))
      .mockRejectedValueOnce({ kind: 'io', data: { message: 'previous request failed' } })
      .mockResolvedValueOnce(checkResponse({ ...updateInfo('cannotCheck'), error }, 'notCompleted'));
    await useLibraryUpdateWorkflow.getState().check();
    await useLibraryUpdateWorkflow.getState().check();
    await useLibraryUpdateWorkflow.getState().check();
    expect(useLibraryUpdateWorkflow.getState()).toMatchObject({
      checks: { demo: { status: 'updateAvailable', error } },
      hasError: true,
      error: null,
    });
  });

  it('clears an operation error when retry starts and keeps the last comparison until completion', async () => {
    let resolve!: (response: UpdateCheckResponse) => void;
    const error = { kind: 'io' as const, data: { message: 'request failed' } };
    mocks.checkLibrarySkillUpdates
      .mockResolvedValueOnce(checkResponse(updateInfo('updateAvailable'), 'completed'))
      .mockRejectedValueOnce(error)
      .mockReturnValueOnce(new Promise((done) => { resolve = done; }));
    await useLibraryUpdateWorkflow.getState().check();
    await useLibraryUpdateWorkflow.getState().check();
    expect(useLibraryUpdateWorkflow.getState().error).toEqual(error);

    const retry = useLibraryUpdateWorkflow.getState().check();
    const duringRetry = useLibraryUpdateWorkflow.getState();
    resolve(checkResponse(updateInfo('upToDate'), 'completed'));
    await retry;

    expect(duringRetry).toMatchObject({
      phase: 'checking',
      error: null,
      hasError: false,
      checks: { demo: { status: 'updateAvailable', hasUpdate: true } },
    });
    expect(useLibraryUpdateWorkflow.getState()).toMatchObject({
      phase: 'idle',
      error: null,
      hasError: false,
      checks: { demo: { status: 'upToDate', hasUpdate: false } },
    });
  });

  it('prepares all download hosts before executing exactly once', async () => {
    mocks.prepareLibrarySkillUpdates.mockResolvedValue({ ...preview, redirectedDownloadHosts: ['cdn.example.com', 'assets.example.com'] });
    mocks.updateLibrarySkills.mockResolvedValue(completed());
    await useLibraryUpdateWorkflow.getState().prepare(['demo']);
    const pending = useLibraryUpdateWorkflow.getState().pending!;
    expect(pending.redirectedDownloadHosts).toHaveLength(2);
    expect(mocks.updateLibrarySkills).not.toHaveBeenCalled();
    expect(await useLibraryUpdateWorkflow.getState().confirm()).not.toBeNull();
    expect(mocks.updateLibrarySkills).toHaveBeenCalledExactlyOnceWith(pending.operationId);
    expect(useLibraryUpdateWorkflow.getState().pending).toBeNull();
  });

  it('releases a cancelled preparing operation and ignores its late result', async () => {
    let resolve!: (value: PreparedLibraryUpdatePreview) => void;
    mocks.prepareLibrarySkillUpdates.mockReturnValue(new Promise((done) => { resolve = done; }));
    const preparing = useLibraryUpdateWorkflow.getState().prepare(['demo']);
    const id = useLibraryUpdateWorkflow.getState().pending!.operationId;
    useLibraryUpdateWorkflow.getState().cancel();
    expect(mocks.cancelUpdatePreparation).toHaveBeenCalledWith(id);
    resolve(preview);
    await preparing;
    expect(useLibraryUpdateWorkflow.getState()).toMatchObject({ phase: 'idle', pending: null });
    expect(mocks.updateLibrarySkills).not.toHaveBeenCalled();
  });

  it('keeps blocked members visible and prevents an empty execution', async () => {
    mocks.prepareLibrarySkillUpdates.mockResolvedValue({ skillNames: [], redirectedDownloadHosts: [], blocked: [{ skillName: 'demo', error: { kind: 'staleTarget' } }] });
    await useLibraryUpdateWorkflow.getState().prepare(['demo']);
    await useLibraryUpdateWorkflow.getState().confirm();
    expect(useLibraryUpdateWorkflow.getState().pending?.blocked).toHaveLength(1);
    expect(mocks.updateLibrarySkills).not.toHaveBeenCalled();
  });

  it('completes when only the post-commit library snapshot fails', async () => {
    mocks.prepareLibrarySkillUpdates.mockResolvedValue(preview);
    mocks.updateLibrarySkills.mockResolvedValue({ ...completed(), membership: { scopes: [], cleanup: [], snapshotError: { kind: 'io', data: { message: 'snapshot unavailable' } } } });
    await useLibraryUpdateWorkflow.getState().prepare(['demo']);
    const response = await useLibraryUpdateWorkflow.getState().confirm();
    expect(response?.library).toBeNull();
    expect(useLibraryUpdateWorkflow.getState()).toMatchObject({ phase: 'idle', pending: null, hasError: false, lastResults: { demo: { status: 'succeeded' } } });
  });

  it('does not let an old check overwrite another library or clear its new error', async () => {
    let resolve!: (value: UpdateCheckResponse) => void;
    const error = { kind: 'io' as const, data: { message: 'new library unavailable' } };
    mocks.checkLibrarySkillUpdates
      .mockReturnValueOnce(new Promise((done) => { resolve = done; }))
      .mockRejectedValueOnce(error);
    const checking = useLibraryUpdateWorkflow.getState().check();
    useLibraryUpdateWorkflow.getState().activate(environment, 'library-2');
    await useLibraryUpdateWorkflow.getState().check();
    resolve(checkResponse(updateInfo('updateAvailable'), 'completed'));
    await checking;
    expect(useLibraryUpdateWorkflow.getState()).toMatchObject({ libraryId: 'library-2', checks: {}, hasError: true, error });
  });

  it('does not report static library members as check failures', async () => {
    mocks.checkLibrarySkillUpdates.mockResolvedValue({ outcome: 'notCompleted', skills: [], sources: [] });
    await useLibraryUpdateWorkflow.getState().check();
    expect(useLibraryUpdateWorkflow.getState().hasError).toBe(false);
  });
});
