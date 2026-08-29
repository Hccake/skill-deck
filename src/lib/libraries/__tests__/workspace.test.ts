import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createLibraryWorkspace } from '../workspace';
import type { LibraryWorkspaceResult } from '../workspace';
import type {
  AcquiredPayloadHandle,
  FetchResult,
  LibraryAddPreview,
  LibraryWorkspaceSnapshot,
  SkillLibraryDetail,
} from '@/bindings';

const api = vi.hoisted(() => ({
  listSkillLibraries: vi.fn(),
  createSkillLibrary: vi.fn(),
  renameSkillLibrary: vi.fn(),
  deleteSkillLibrary: vi.fn(),
  getSkillLibrary: vi.fn(),
  acquireSelectedPayloads: vi.fn(),
  previewAddLibrarySkills: vi.fn(),
  addSkillsToLibrary: vi.fn(),
}));
const write = vi.hoisted(() => ({
  runBusinessWrite: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => api);
vi.mock('@/workflows/install-session-feedback', () => write);

const environment = { kind: 'native' } as const;
const emptyCatalog: LibraryWorkspaceSnapshot = {
  environment,
  libraries: [],
  revision: 'catalog-empty',
  usageProjection: [],
};
const createdCatalog: LibraryWorkspaceSnapshot = {
  environment,
  libraries: [{ id: 'lib-1', name: 'Backend', skillCount: 0 }],
  revision: 'catalog-created',
  usageProjection: [],
};
const threeLibraryCatalog: LibraryWorkspaceSnapshot = {
  environment,
  libraries: [
    { id: 'lib-a', name: 'A', skillCount: 1 },
    { id: 'lib-b', name: 'B', skillCount: 2 },
    { id: 'lib-c', name: 'C', skillCount: 3 },
  ],
  revision: 'catalog-three',
  usageProjection: [],
};
const detailFor = (id: string): SkillLibraryDetail => ({
  id,
  name: id.toUpperCase(),
  skills: [],
  usages: [],
});
const emptyDetail: SkillLibraryDetail = { id: 'lib-1', name: 'Backend', skills: [], usages: [] };
const previewToken = (generation: string) => ({
  generation,
  contextRevision: `context-${generation}`,
  skillRevisions: [],
  redirectedDownloadHost: null,
});
const libraryPreview = (
  generation: string,
  skills: LibraryAddPreview['skills'],
): LibraryAddPreview => ({
  token: previewToken(generation),
  skills,
  redirectedDownloadHost: null,
});

function succeeded(result: LibraryWorkspaceResult) {
  expect(result.status).toBe('succeeded');
  if (result.status !== 'succeeded') throw new Error('workspace command should succeed');
  return result.snapshot;
}

describe('LibraryWorkspace', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.listSkillLibraries.mockResolvedValue(emptyCatalog);
    api.createSkillLibrary.mockResolvedValue(createdCatalog);
    api.getSkillLibrary.mockResolvedValue(emptyDetail);
    write.runBusinessWrite.mockImplementation(async (operation: () => Promise<unknown>) => ({
      status: 'completed',
      value: await operation(),
    }));
  });

  it('creates and selects a Library through one workspace command', async () => {
    const workspace = createLibraryWorkspace();

    const result = await workspace.execute({
      kind: 'create',
      environment,
      name: 'Backend',
    });

    expect(result.status).toBe('succeeded');
    if (result.status !== 'succeeded') throw new Error('create should succeed');
    const snapshot = result.snapshot;
    expect(snapshot.phase).toBe('ready');
    expect(snapshot.selectedLibraryId).toBe('lib-1');
    expect(snapshot.detail).toEqual(emptyDetail);
    expect(api.createSkillLibrary).toHaveBeenCalledWith(environment, 'Backend');
    expect(api.getSkillLibrary).toHaveBeenCalledWith(environment, 'lib-1');
  });

  it('keeps the last ready snapshot when rename fails', async () => {
    api.listSkillLibraries.mockResolvedValue(createdCatalog);
    api.getSkillLibrary.mockResolvedValue(emptyDetail);
    api.renameSkillLibrary.mockRejectedValue({ kind: 'staleTarget' });
    const workspace = createLibraryWorkspace();
    succeeded(await workspace.execute({ kind: 'load', environment }));

    const result = await workspace.execute({
      kind: 'rename', environment, libraryId: 'lib-1', name: 'Renamed',
    });

    expect(result.status).toBe('failed');
    if (result.status !== 'failed') throw new Error('rename should fail');
    expect(result.failureSource).toBe('command');
    expect(result.snapshot.phase).toBe('ready');
    expect(result.snapshot.catalog).toBe(createdCatalog);
    expect(result.snapshot.detail).toBe(emptyDetail);
  });

  it('deletes an unselected Library without changing the current selection or detail', async () => {
    const catalog = threeLibraryCatalog;
    const detailA = { id: 'lib-a', name: 'A', skills: [], usages: [] } satisfies SkillLibraryDetail;
    api.listSkillLibraries.mockResolvedValue(catalog);
    api.getSkillLibrary.mockResolvedValue(detailA);
    api.deleteSkillLibrary.mockResolvedValue({
      ...catalog,
      libraries: [catalog.libraries[0], catalog.libraries[2]],
      revision: 'catalog-after-delete',
    });
    const workspace = createLibraryWorkspace();
    succeeded(await workspace.execute({ kind: 'load', environment }));
    vi.clearAllMocks();

    const result = await workspace.execute({
      kind: 'delete',
      environment,
      libraryId: 'lib-b',
    });

    const snapshot = succeeded(result);
    expect(api.deleteSkillLibrary).toHaveBeenCalledWith(environment, 'lib-b');
    expect(api.listSkillLibraries).not.toHaveBeenCalled();
    expect(api.getSkillLibrary).not.toHaveBeenCalled();
    expect(snapshot.selectedLibraryId).toBe('lib-a');
    expect(snapshot.detail).toBe(detailA);
    expect(snapshot.catalog?.libraries.map((library) => library.id)).toEqual(['lib-a', 'lib-c']);
  });

  it('selects the next Library after deleting the current middle item', async () => {
    api.listSkillLibraries.mockResolvedValue(threeLibraryCatalog);
    api.getSkillLibrary.mockImplementation(async (_environment, id) => detailFor(id));
    api.deleteSkillLibrary.mockResolvedValue({
      ...threeLibraryCatalog,
      libraries: [threeLibraryCatalog.libraries[0], threeLibraryCatalog.libraries[2]],
      revision: 'catalog-without-b',
    });
    const workspace = createLibraryWorkspace();
    succeeded(await workspace.execute({ kind: 'load', environment }));
    succeeded(await workspace.execute({ kind: 'select', environment, libraryId: 'lib-b' }));
    vi.clearAllMocks();

    const snapshot = succeeded(await workspace.execute({
      kind: 'delete', environment, libraryId: 'lib-b',
    }));

    expect(snapshot.selectedLibraryId).toBe('lib-c');
    expect(snapshot.detail?.id).toBe('lib-c');
    expect(api.getSkillLibrary).toHaveBeenCalledWith(environment, 'lib-c');
  });

  it('selects the previous Library after deleting the current final item', async () => {
    api.listSkillLibraries.mockResolvedValue(threeLibraryCatalog);
    api.getSkillLibrary.mockImplementation(async (_environment, id) => detailFor(id));
    api.deleteSkillLibrary.mockResolvedValue({
      ...threeLibraryCatalog,
      libraries: [threeLibraryCatalog.libraries[0], threeLibraryCatalog.libraries[1]],
      revision: 'catalog-without-c',
    });
    const workspace = createLibraryWorkspace();
    succeeded(await workspace.execute({ kind: 'load', environment }));
    succeeded(await workspace.execute({ kind: 'select', environment, libraryId: 'lib-c' }));

    const snapshot = succeeded(await workspace.execute({
      kind: 'delete', environment, libraryId: 'lib-c',
    }));

    expect(snapshot.selectedLibraryId).toBe('lib-b');
    expect(snapshot.detail?.id).toBe('lib-b');
  });

  it('clears selection after deleting the only Library', async () => {
    api.listSkillLibraries.mockResolvedValue(createdCatalog);
    api.getSkillLibrary.mockResolvedValue(emptyDetail);
    api.deleteSkillLibrary.mockResolvedValue(emptyCatalog);
    const workspace = createLibraryWorkspace();
    succeeded(await workspace.execute({ kind: 'load', environment }));

    const snapshot = succeeded(await workspace.execute({
      kind: 'delete', environment, libraryId: 'lib-1',
    }));

    expect(snapshot.selectedLibraryId).toBeNull();
    expect(snapshot.detail).toBeNull();
    expect(snapshot.detailPhase).toBe('idle');
  });

  it('keeps the previous snapshot when Library deletion fails', async () => {
    api.listSkillLibraries.mockResolvedValue(createdCatalog);
    api.getSkillLibrary.mockResolvedValue(emptyDetail);
    api.deleteSkillLibrary.mockRejectedValue({ kind: 'staleTarget' });
    const workspace = createLibraryWorkspace();
    succeeded(await workspace.execute({ kind: 'load', environment }));

    const result = await workspace.execute({
      kind: 'delete', environment, libraryId: 'lib-1',
    });

    expect(result.status).toBe('failed');
    if (result.status !== 'failed') throw new Error('delete should fail');
    expect(result.failureSource).toBe('command');
    expect(result.snapshot.phase).toBe('ready');
    expect(result.snapshot.catalog).toBe(createdCatalog);
    expect(result.snapshot.detail).toBe(emptyDetail);
  });

  it('does not run deletion while another write flow owns admission', async () => {
    api.listSkillLibraries.mockResolvedValue(createdCatalog);
    api.getSkillLibrary.mockResolvedValue(emptyDetail);
    write.runBusinessWrite.mockResolvedValue({
      status: 'notRun',
      reason: 'installFlowActive',
    });
    const workspace = createLibraryWorkspace();
    succeeded(await workspace.execute({ kind: 'load', environment }));

    const result = await workspace.execute({
      kind: 'delete', environment, libraryId: 'lib-1',
    });

    expect(result).toEqual({ status: 'notRun', reason: 'writeBlocked' });
    expect(api.deleteSkillLibrary).not.toHaveBeenCalled();
    expect(workspace.getSnapshot(environment).phase).toBe('ready');
    expect(workspace.getSnapshot(environment).selectedLibraryId).toBe('lib-1');
  });

  it('keeps deletion successful when loading the fallback detail fails', async () => {
    api.listSkillLibraries.mockResolvedValue(threeLibraryCatalog);
    api.getSkillLibrary.mockImplementation(async (_environment, id) => detailFor(id));
    api.deleteSkillLibrary.mockResolvedValue({
      ...threeLibraryCatalog,
      libraries: [threeLibraryCatalog.libraries[0], threeLibraryCatalog.libraries[2]],
      revision: 'catalog-without-b',
    });
    const workspace = createLibraryWorkspace();
    succeeded(await workspace.execute({ kind: 'load', environment }));
    succeeded(await workspace.execute({ kind: 'select', environment, libraryId: 'lib-b' }));
    api.getSkillLibrary.mockRejectedValueOnce({ kind: 'io', data: { message: 'unavailable' } });

    const snapshot = succeeded(await workspace.execute({
      kind: 'delete', environment, libraryId: 'lib-b',
    }));

    expect(snapshot.catalog?.libraries.map((library) => library.id)).toEqual(['lib-a', 'lib-c']);
    expect(snapshot.selectedLibraryId).toBe('lib-c');
    expect(snapshot.detail).toBeNull();
    expect(snapshot.detailPhase).toBe('error');
    expect(snapshot.detailError).toEqual({ kind: 'io', data: { message: 'unavailable' } });
  });

  it('pins and adds discovered Skills before refreshing the catalog', async () => {
    const fetched = {
      discoverySession: {
        sessionId: 'session-1',
        environment,
        sourceFingerprint: 'source-1',
        expiresAtEpochMs: 10_000,
      },
      sourceType: 'git',
      sourceUrl: 'https://example.com/repo',
      redirectedDownloadHost: null,
      gitRef: null,
      skillFilter: null,
      skills: [{
        name: 'api-design',
        installDirName: 'api-design',
        description: 'Design APIs',
        relativePath: 'skills/api-design',
      }],
    } satisfies FetchResult;
    const handle = {
      sessionId: 'session-1',
      skillPath: 'skills/api-design',
      environment,
      payloadId: 'payload-1',
      manifestHash: 'manifest-1',
      sourceFingerprint: 'source-1',
      expiresAtEpochMs: 10_000,
    } satisfies AcquiredPayloadHandle;
    const detail: SkillLibraryDetail = {
      id: 'lib-1',
      name: 'Backend',
      usages: [],
      skills: [{
        name: 'api-design',
        description: 'Design APIs',
        source: 'https://example.com/repo',
        sourceType: 'git',
        sourceUrl: 'https://example.com/repo',
        skillPath: 'skills/api-design',
        pluginName: null,
  refName: null,
  contentHash: 'manifest-1',
  updatedAt: null,
      }],
    };
    api.acquireSelectedPayloads.mockResolvedValue([handle]);
    const preview = libraryPreview('preview-1', [
      { skillName: 'api-design', targetPath: '/libraries/lib-1/skills/api-design' },
    ]);
    api.previewAddLibrarySkills.mockResolvedValue(preview);
    api.addSkillsToLibrary.mockResolvedValue({
      results: [{ skillName: 'api-design', status: 'succeeded', error: null }],
      library: detail,
    });
    api.listSkillLibraries.mockResolvedValue({
      ...createdCatalog,
      libraries: [{ ...createdCatalog.libraries[0], skillCount: 1 }],
    });
    const workspace = createLibraryWorkspace();

    const prepared = succeeded(await workspace.execute({
      kind: 'addSkills',
      environment,
      libraryId: 'lib-1',
      discovery: fetched,
    }));

    expect(prepared.pendingAdd?.preview).toEqual(preview);
    const snapshot = succeeded(await workspace.execute({
      kind: 'confirmAddSkills',
      environment,
      acknowledgeRedirect: false,
    }));
    expect(snapshot.detail).toEqual(detail);
    expect(api.acquireSelectedPayloads).toHaveBeenCalledWith({
      discoverySession: fetched.discoverySession,
      skillPaths: ['skills/api-design'],
    });
    expect(api.addSkillsToLibrary).toHaveBeenCalledWith({
      request: {
        environment,
        libraryId: 'lib-1',
        discoverySession: fetched.discoverySession,
        skills: [{ skillName: 'api-design', payload: handle }],
      },
      expectedToken: preview.token,
      acknowledgeRedirect: false,
    });
  });

  it('reuses an inspected source when adding only the selected Skills', async () => {
    const fetched = {
      discoverySession: {
        sessionId: 'session-1',
        environment,
        sourceFingerprint: 'source-1',
        expiresAtEpochMs: 10_000,
      },
      sourceType: 'git',
      sourceUrl: 'https://example.com/repo',
      redirectedDownloadHost: null,
      gitRef: null,
      skillFilter: null,
      skills: [
        { name: 'api', installDirName: 'api', description: 'API', relativePath: 'skills/api' },
        { name: 'ui', installDirName: 'ui', description: 'UI', relativePath: 'skills/ui' },
      ],
    } satisfies FetchResult;
    api.acquireSelectedPayloads.mockResolvedValue([]);
    api.previewAddLibrarySkills.mockResolvedValue(libraryPreview('preview-2', []));
    const workspace = createLibraryWorkspace();

    await workspace.execute({
      kind: 'addSkills',
      environment,
      libraryId: 'lib-1',
      discovery: fetched,
      skillPaths: ['skills/ui'],
    });

    expect(api.acquireSelectedPayloads).toHaveBeenCalledWith({
      discoverySession: fetched.discoverySession,
      skillPaths: ['skills/ui'],
    });
  });

  it('prepares a fresh preview containing only failed Skills after a partial batch result', async () => {
    const fetched = {
      discoverySession: {
        sessionId: 'session-1',
        environment,
        sourceFingerprint: 'source-1',
        expiresAtEpochMs: 10_000,
      },
      sourceType: 'git',
      sourceUrl: 'https://example.com/repo',
      redirectedDownloadHost: null,
      gitRef: null,
      skillFilter: null,
      skills: [
        { name: 'api', installDirName: 'api', description: 'API', relativePath: 'skills/api' },
        { name: 'ui', installDirName: 'ui', description: 'UI', relativePath: 'skills/ui' },
      ],
    } satisfies FetchResult;
    const handles = ['api', 'ui'].map((name) => ({
      sessionId: 'session-1',
      skillPath: `skills/${name}`,
      environment,
      payloadId: `payload-${name}`,
      manifestHash: `manifest-${name}`,
      sourceFingerprint: 'source-1',
      expiresAtEpochMs: 10_000,
    })) satisfies AcquiredPayloadHandle[];
    api.acquireSelectedPayloads.mockResolvedValue(handles);
    api.previewAddLibrarySkills
      .mockResolvedValueOnce(libraryPreview('preview-1', [
          { skillName: 'api', targetPath: '/libraries/lib-1/skills/api' },
          { skillName: 'ui', targetPath: '/libraries/lib-1/skills/ui' },
      ]))
      .mockResolvedValueOnce(libraryPreview('preview-retry', [
        { skillName: 'ui', targetPath: '/libraries/lib-1/skills/ui' },
      ]));
    api.addSkillsToLibrary.mockResolvedValue({
      results: [
        { skillName: 'api', status: 'succeeded', error: null },
        { skillName: 'ui', status: 'failed', error: { kind: 'staleTarget' } },
      ],
      library: emptyDetail,
    });
    api.listSkillLibraries.mockResolvedValue(createdCatalog);
    const workspace = createLibraryWorkspace();

    await workspace.execute({
      kind: 'addSkills',
      environment,
      libraryId: 'lib-1',
      discovery: fetched,
    });
    const snapshot = succeeded(await workspace.execute({
      kind: 'confirmAddSkills',
      environment,
      acknowledgeRedirect: false,
    }));

    expect(snapshot.lastAddResults).toHaveLength(2);
    expect(snapshot.pendingAdd?.request.skills).toEqual([{ skillName: 'ui', payload: handles[1] }]);
    expect(api.previewAddLibrarySkills).toHaveBeenLastCalledWith({
      environment,
      libraryId: 'lib-1',
      discoverySession: fetched.discoverySession,
      skills: [{ skillName: 'ui', payload: handles[1] }],
    });
  });

  it('keeps a failed retry preview visible and lets the user prepare it again', async () => {
    const fetched = {
      discoverySession: {
        sessionId: 'session-1',
        environment,
        sourceFingerprint: 'source-1',
        expiresAtEpochMs: 10_000,
      },
      sourceType: 'git',
      sourceUrl: 'https://example.com/repo',
      redirectedDownloadHost: null,
      gitRef: null,
      skillFilter: null,
      skills: [{ name: 'ui', installDirName: 'ui', description: 'UI', relativePath: 'skills/ui' }],
    } satisfies FetchResult;
    const handle = {
      sessionId: 'session-1',
      skillPath: 'skills/ui',
      environment,
      payloadId: 'payload-ui',
      manifestHash: 'manifest-ui',
      sourceFingerprint: 'source-1',
      expiresAtEpochMs: 10_000,
    } satisfies AcquiredPayloadHandle;
    api.acquireSelectedPayloads.mockResolvedValue([handle]);
    api.previewAddLibrarySkills
      .mockResolvedValueOnce(libraryPreview('preview-1', [
        { skillName: 'ui', targetPath: '/libraries/lib-1/skills/ui' },
      ]))
      .mockRejectedValueOnce({ kind: 'stalePayload' });
    api.addSkillsToLibrary.mockResolvedValue({
      results: [{ skillName: 'ui', status: 'failed', error: { kind: 'staleTarget' } }],
      library: emptyDetail,
    });
    api.listSkillLibraries.mockResolvedValue(createdCatalog);
    const workspace = createLibraryWorkspace();

    await workspace.execute({
      kind: 'addSkills',
      environment,
      libraryId: 'lib-1',
      discovery: fetched,
    });
    const failed = succeeded(await workspace.execute({
      kind: 'confirmAddSkills',
      environment,
      acknowledgeRedirect: false,
    }));

    expect(failed.retryAdd?.error).toEqual({ kind: 'stalePayload' });
    expect(failed.lastAddResults).toHaveLength(1);

    api.previewAddLibrarySkills.mockResolvedValueOnce(libraryPreview('preview-2', [
      { skillName: 'ui', targetPath: '/libraries/lib-1/skills/ui' },
    ]));
    const retried = succeeded(await workspace.execute({ kind: 'retryAddPreview', environment }));

    expect(retried.pendingAdd?.preview.token.generation).toBe('preview-2');
    expect(retried.retryAdd).toBeNull();
    expect(retried.lastAddResults).toEqual([]);
  });

  it('ignores an add preview that resolves after a newer command in the same Environment', async () => {
    const fetched = {
      discoverySession: {
        sessionId: 'session-1',
        environment,
        sourceFingerprint: 'source-1',
        expiresAtEpochMs: 10_000,
      },
      sourceType: 'git',
      sourceUrl: 'https://example.com/repo',
      redirectedDownloadHost: null,
      gitRef: null,
      skillFilter: null,
      skills: [{ name: 'ui', installDirName: 'ui', description: 'UI', relativePath: 'skills/ui' }],
    } satisfies FetchResult;
    api.acquireSelectedPayloads.mockResolvedValue([{
      sessionId: 'session-1',
      skillPath: 'skills/ui',
      environment,
      payloadId: 'payload-ui',
      manifestHash: 'manifest-ui',
      sourceFingerprint: 'source-1',
      expiresAtEpochMs: 10_000,
    } satisfies AcquiredPayloadHandle]);
    let resolvePreview!: (preview: LibraryAddPreview) => void;
    api.previewAddLibrarySkills.mockReturnValueOnce(new Promise((resolve) => {
      resolvePreview = resolve;
    }));
    const workspace = createLibraryWorkspace();

    const stale = workspace.execute({
      kind: 'addSkills',
      environment,
      libraryId: 'lib-1',
      discovery: fetched,
    });
    await vi.waitFor(() => expect(api.previewAddLibrarySkills).toHaveBeenCalled());
    await workspace.execute({ kind: 'load', environment });
    resolvePreview(libraryPreview('stale-preview', [
      { skillName: 'ui', targetPath: '/libraries/lib-1/skills/ui' },
    ]));
    await stale;

    expect(workspace.getSnapshot(environment).pendingAdd).toBeNull();
  });

  it('keeps Native and WSL Library add state isolated', async () => {
    const fetched = {
      discoverySession: {
        sessionId: 'session-native',
        environment,
        sourceFingerprint: 'source-native',
        expiresAtEpochMs: 10_000,
      },
      sourceType: 'git',
      sourceUrl: 'https://example.com/repo',
      redirectedDownloadHost: null,
      gitRef: null,
      skillFilter: null,
      skills: [{ name: 'ui', installDirName: 'ui', description: 'UI', relativePath: 'skills/ui' }],
    } satisfies FetchResult;
    api.acquireSelectedPayloads.mockResolvedValue([{
      sessionId: 'session-native',
      skillPath: 'skills/ui',
      environment,
      payloadId: 'payload-ui',
      manifestHash: 'manifest-ui',
      sourceFingerprint: 'source-native',
      expiresAtEpochMs: 10_000,
    } satisfies AcquiredPayloadHandle]);
    api.previewAddLibrarySkills.mockResolvedValue(libraryPreview('native-preview', [
      { skillName: 'ui', targetPath: '/libraries/lib-1/skills/ui' },
    ]));
    const wsl = { kind: 'wsl', distro_name: 'Ubuntu' } as const;
    api.listSkillLibraries.mockResolvedValueOnce({
      environment: wsl,
      libraries: [],
      revision: 'wsl-empty',
    });
    const workspace = createLibraryWorkspace();

    await workspace.execute({
      kind: 'addSkills',
      environment,
      libraryId: 'lib-1',
      discovery: fetched,
    });
    await workspace.execute({ kind: 'load', environment: wsl });

    expect(workspace.getSnapshot(environment).pendingAdd?.preview.token.generation)
      .toBe('native-preview');
    expect(workspace.getSnapshot(wsl).pendingAdd).toBeNull();
  });
});
