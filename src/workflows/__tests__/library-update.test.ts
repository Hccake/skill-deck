import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  LibraryUpdateContinuation,
  LibraryUpdatePreview,
  SkillUpdateInfo,
  UpdateCheckResponse,
} from '@/bindings';

const mocks = vi.hoisted(() => ({
  checkLibrarySkillUpdates: vi.fn(),
  previewLibrarySkillUpdates: vi.fn(),
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
});
const checkResponse = (skill: SkillUpdateInfo, outcome: UpdateCheckResponse['outcome']): UpdateCheckResponse => ({
  outcome,
  sources: [],
  skills: [skill],
});
const preview: LibraryUpdatePreview = {
  token: { generation: 'preview-1' },
  skillNames: ['demo'],
};

describe('library update workflow', () => {
  beforeEach(() => {
    useLibraryUpdateWorkflow.getState().reset();
    mocks.checkLibrarySkillUpdates.mockReset();
    mocks.previewLibrarySkillUpdates.mockReset();
    mocks.updateLibrarySkills.mockReset();
    useLibraryUpdateWorkflow.getState().activate(environment, 'library-1');
  });

  it('keeps the last successful member status when a refresh cannot check it', async () => {
    mocks.checkLibrarySkillUpdates
      .mockResolvedValueOnce(checkResponse(updateInfo('updateAvailable'), 'completed'))
      .mockResolvedValueOnce(checkResponse(updateInfo('cannotCheck'), 'notCompleted'));

    await useLibraryUpdateWorkflow.getState().check();
    await useLibraryUpdateWorkflow.getState().check();

    expect(useLibraryUpdateWorkflow.getState()).toMatchObject({
      checks: { demo: { status: 'updateAvailable' } },
      hasError: true,
    });
  });

  it('reuses the prepared batch after redirect confirmation', async () => {
    const continuation = { sources: [] } satisfies LibraryUpdateContinuation;
    mocks.previewLibrarySkillUpdates.mockResolvedValue(preview);
    mocks.updateLibrarySkills
      .mockResolvedValueOnce({
        status: 'confirmationRequired',
        token: { generation: 'confirmed-preview' },
        redirectedDownloadHosts: ['cdn.example.com'],
        continuation,
      })
      .mockResolvedValueOnce({
        status: 'completed',
        response: { sources: [], results: [], outcome: 'succeeded', library: { id: 'library-1', name: 'Tools', skills: [], usages: [] } },
      });

    await useLibraryUpdateWorkflow.getState().prepare(['demo']);
    expect(await useLibraryUpdateWorkflow.getState().confirm()).toBeNull();
    expect(await useLibraryUpdateWorkflow.getState().confirm()).not.toBeNull();

    expect(mocks.updateLibrarySkills).toHaveBeenLastCalledWith({
      request: { environment, libraryId: 'library-1', skillNames: ['demo'] },
      expectedToken: { generation: 'confirmed-preview' },
      continuation,
      riskConfirmation: { redirectedDownloadHosts: ['cdn.example.com'] },
    });
  });

  it('does not let an old check overwrite a newly activated Library', async () => {
    let resolveCheck!: (value: UpdateCheckResponse) => void;
    mocks.checkLibrarySkillUpdates.mockReturnValue(new Promise((resolve) => { resolveCheck = resolve; }));

    const pending = useLibraryUpdateWorkflow.getState().check();
    useLibraryUpdateWorkflow.getState().activate(environment, 'library-2');
    resolveCheck(checkResponse(updateInfo('updateAvailable'), 'completed'));
    await pending;

    expect(useLibraryUpdateWorkflow.getState()).toMatchObject({
      libraryId: 'library-2',
      checks: {},
    });
  });
});
