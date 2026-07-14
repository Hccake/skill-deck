import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, InstalledSkill, ListSkillsResult } from '@/bindings';
import { contextKey, globalContext } from '@/lib/context';
import { useSkillsDataStore } from '../skills-data';

const mocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  listAgents: vi.fn(),
  checkUpdates: vi.fn(),
  updateSkill: vi.fn(),
  updateSkillsBatch: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mocks.listSkills(...args),
  listAgents: (...args: unknown[]) => mocks.listAgents(...args),
  checkUpdates: (...args: unknown[]) => mocks.checkUpdates(...args),
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
    mocks.listAgents.mockResolvedValue([]);
    mocks.listSkills.mockResolvedValue({ skills: [], pathExists: true });
    mocks.checkUpdates.mockResolvedValue([]);
    mocks.updateSkill.mockResolvedValue({
      results: [{ name: 'toolkit', status: 'success', warnings: [], agentResults: [] }],
    });
    mocks.updateSkillsBatch.mockResolvedValue({
      results: [{ name: 'toolkit', status: 'success', warnings: [], agentResults: [] }],
    });
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
    mocks.listSkills.mockImplementation(async (context: ContextRef) => (
      context.scope.scope === 'global' ? result('global') : result('project', 'project')
    ));

    await useSkillsDataStore.getState().refreshWorkspace(ubuntuProject);

    expect(mocks.listSkills).toHaveBeenCalledWith(globalContext(ubuntuGlobal.environment));
    expect(mocks.listSkills).toHaveBeenCalledWith(ubuntuProject);
    expect(mocks.listAgents).toHaveBeenCalledWith(ubuntuProject);
    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuProject)].skills[0].name)
      .toBe('project');
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
    mocks.checkUpdates.mockResolvedValue([{
      name: 'toolkit',
      source: 'owner/repo',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: null,
      skillPath: null,
      hasUpdate: true,
      status: 'update-available',
      reason: null,
    }]);

    await useSkillsDataStore.getState().forceCheckUpdates(ubuntuGlobal);

    expect(mocks.checkUpdates).toHaveBeenCalledWith(ubuntuGlobal);
    expect(useSkillsDataStore.getState().snapshots[contextKey(ubuntuGlobal)].skills[0].hasUpdate)
      .toBe(true);
  });

  it('updates one skill using only the captured context', async () => {
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(ubuntuGlobal)]: {
          skills: [skill('toolkit')],
          agents: [],
          pathExists: true,
          loading: false,
          error: null,
          requestId: 1,
        },
      },
    });

    await useSkillsDataStore.getState().updateSkill(ubuntuGlobal, 'toolkit');

    expect(mocks.updateSkill).toHaveBeenCalledWith(ubuntuGlobal, 'toolkit');
  });

  it('updates a section using only the captured context', async () => {
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(ubuntuGlobal)]: {
          skills: [skill('toolkit')],
          agents: [],
          pathExists: true,
          loading: false,
          error: null,
          requestId: 1,
        },
      },
    });
    const current = useSkillsDataStore.getState().snapshots[contextKey(ubuntuGlobal)];
    current.skills[0] = {
      ...current.skills[0],
      source: 'owner/repo',
      sourceUrl: 'https://github.com/owner/repo',
      hasUpdate: true,
      canRunUpdate: true,
    };

    await useSkillsDataStore.getState().updateAllInSection(ubuntuGlobal);

    expect(mocks.updateSkillsBatch).toHaveBeenCalledWith(ubuntuGlobal, ['toolkit']);
  });
});
