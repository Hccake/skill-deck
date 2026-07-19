import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, InstalledSkill, ListSkillsResult } from '@/bindings';
import { contextKey, globalContext } from '@/lib/context';
import { useSkillsDataStore } from '../skills-data';
import { makeAgentRuntimeSnapshot, makeResolvedAgent } from '@/test-utils';

const mocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  listAgents: vi.fn(),
  checkUpdates: vi.fn(),
  previewUpdate: vi.fn(),
  updateSkill: vi.fn(),
  updateSkillsBatch: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mocks.listSkills(...args),
  listAgents: (...args: unknown[]) => mocks.listAgents(...args),
  checkUpdates: (...args: unknown[]) => mocks.checkUpdates(...args),
  previewUpdate: (...args: unknown[]) => mocks.previewUpdate(...args),
  updateSkill: (...args: unknown[]) => mocks.updateSkill(...args),
  updateSkillsBatch: (...args: unknown[]) => mocks.updateSkillsBatch(...args),
  checkSkillAudit: vi.fn(),
}));

const ubuntuGlobal: ContextRef = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  scope: { scope: 'global' },
};
const ubuntuProject: ContextRef = {
  environment: ubuntuGlobal.environment,
  scope: { scope: 'project', project_id: 'project-a' },
};
const debianGlobal: ContextRef = {
  environment: { kind: 'wsl', distro_name: 'Debian' },
  scope: { scope: 'global' },
};
const previewToken = {
  generation: 'preview-1',
  registryRevision: 'registry-1',
  environmentRevision: 'environment-1',
  contextRevision: 'context-1',
};
function skill(name: string, scope: 'global' | 'project' = 'global'): InstalledSkill {
  return {
    name,
    description: `${name} description`,
    path: `/skills/${name}`,
    canonicalPath: `/canonical/${name}`,
    scope,
    agents: [],
    hasUpdate: false,
  };
}

function result(name: string, scope: 'global' | 'project' = 'global'): ListSkillsResult {
  return { skills: [skill(name, scope)], pathExists: true };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe('context-keyed Skill snapshots', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSkillsDataStore.setState({ snapshots: {} });
    mocks.listAgents.mockResolvedValue(makeAgentRuntimeSnapshot([]));
    mocks.listSkills.mockResolvedValue({ skills: [], pathExists: true });
    mocks.checkUpdates.mockResolvedValue({ sources: [], skills: [] });
    mocks.previewUpdate.mockResolvedValue({
      token: previewToken,
      skills: [{
        skillName: 'toolkit',
        capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
        overwritePrivateEntries: [{ entryId: 'entry-1' }],
        blockingReasons: [],
        fallbackForecasts: [],
      }],
    });
    const unit = {
      unitId: 'toolkit',
      source: null,
      target: ubuntuGlobal,
      status: 'succeeded',
      retryable: false,
      lockCommitted: true,
      actualMode: 'copy',
      fallbackReason: null,
      agentTargets: [],
      warnings: [],
      error: null,
      recovery: null,
    };
    const response = {
      sources: [{ id: 'source-1', source: 'owner/repo', status: 'acquired', error: null }],
      skills: [{
        skillIdentity: { context: ubuntuGlobal, skillName: 'toolkit' },
        sourceResultId: 'source-1',
        mutation: unit,
        coverage: { kind: 'updated' },
        warnings: [],
        retryable: false,
      }],
      outcome: 'succeeded',
    };
    mocks.updateSkill.mockResolvedValue(response);
    mocks.updateSkillsBatch.mockResolvedValue(response);
  });

  it('keeps concurrent environment results in independent snapshots', async () => {
    const ubuntu = deferred<ListSkillsResult>();
    const debian = deferred<ListSkillsResult>();
    mocks.listSkills.mockImplementation((context: ContextRef) => (
      context.environment.kind === 'wsl' && context.environment.distro_name === 'Ubuntu'
        ? ubuntu.promise
        : debian.promise
    ));

    const ubuntuLoad = useSkillsDataStore.getState().refreshContext(ubuntuGlobal, false);
    const debianLoad = useSkillsDataStore.getState().refreshContext(debianGlobal, false);
    debian.resolve(result('debian-skill'));
    await debianLoad;
    ubuntu.resolve(result('ubuntu-skill'));
    await ubuntuLoad;

    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuGlobal)].skills[0].name)
      .toBe('ubuntu-skill');
    expect(useSkillsDataStore.getState().snapshots[contextKey(debianGlobal)].skills[0].name)
      .toBe('debian-skill');
  });

  it('keeps a structured AppError when loading a context fails', async () => {
    const error = {
      kind: 'custom',
      data: { message: 'invalid WSL inspect record' },
    } as const;
    mocks.listSkills.mockRejectedValue(error);

    await useSkillsDataStore.getState().refreshContext(ubuntuGlobal, false);

    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuGlobal)].error)
      .toEqual(error);
  });

  it('ignores an older response for the same context key', async () => {
    const first = deferred<ListSkillsResult>();
    const second = deferred<ListSkillsResult>();
    mocks.listSkills
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const firstLoad = useSkillsDataStore.getState().refreshContext(ubuntuGlobal, false);
    const secondLoad = useSkillsDataStore.getState().refreshContext(ubuntuGlobal, false);
    second.resolve(result('new'));
    await secondLoad;
    first.resolve(result('old'));
    await firstLoad;

    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuGlobal)].skills[0].name)
      .toBe('new');
  });

  it('does not let an invalidated request overwrite a newer response', async () => {
    const oldRequest = deferred<ListSkillsResult>();
    const newRequest = deferred<ListSkillsResult>();
    mocks.listSkills
      .mockReturnValueOnce(oldRequest.promise)
      .mockReturnValueOnce(newRequest.promise);

    const oldLoad = useSkillsDataStore.getState().refreshContext(ubuntuGlobal, false);
    useSkillsDataStore.getState().invalidateContexts([ubuntuGlobal]);
    const newLoad = useSkillsDataStore.getState().refreshContext(ubuntuGlobal, false);

    newRequest.resolve(result('new'));
    await newLoad;
    oldRequest.resolve(result('old'));
    await oldLoad;

    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuGlobal)].skills[0].name)
      .toBe('new');
  });

  it('loads project and same-environment Global snapshots in parallel', async () => {
    mocks.listAgents.mockResolvedValue(makeAgentRuntimeSnapshot([
      makeResolvedAgent({ id: 'both' }),
      makeResolvedAgent({
        id: 'global-only',
        project: { enabled: false },
      }),
    ]));
    mocks.listSkills.mockImplementation(async (context: ContextRef) => (
      context.scope.scope === 'global' ? result('global') : result('project', 'project')
    ));

    await useSkillsDataStore.getState().refreshWorkspace(ubuntuProject);

    expect(mocks.listSkills).toHaveBeenCalledWith(globalContext(ubuntuGlobal.environment));
    expect(mocks.listSkills).toHaveBeenCalledWith(ubuntuProject);
    expect(mocks.listAgents).toHaveBeenCalledWith(ubuntuProject);
    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuProject)].skills[0].name)
      .toBe('project');
    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuProject)].agents
      .map((agent) => agent.definition.id)).toEqual(['both']);
  });

  it('checks updates for the captured context and mutates only its snapshot', async () => {
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(ubuntuGlobal)]: {
          ...useSkillsDataStore.getState().snapshots[contextKey(ubuntuGlobal)],
          skills: [{
            ...skill('toolkit'),
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
          }],
          agents: [],
          pathExists: true,
          loading: false,
          error: null,
          requestId: 1,
        },
      },
    });
    mocks.checkUpdates.mockResolvedValue({ sources: [], skills: [{
      name: 'toolkit',
      source: 'owner/repo',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: null,
      skillPath: null,
      hasUpdate: true,
      status: 'updateAvailable',
      reason: null,
      freshness: 'fresh',
      capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
    }] });

    await useSkillsDataStore.getState().forceCheckUpdates(ubuntuGlobal, { kind: 'all' });

    expect(mocks.checkUpdates).toHaveBeenCalledWith({
      context: ubuntuGlobal,
      mode: 'force',
      selection: { kind: 'all' },
    });
    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuGlobal)].skills[0].hasUpdate)
      .toBe(true);
  });

});
