import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstalledSkill } from '@/bindings';

const fixtures = vi.hoisted(() => ({
  read: vi.fn(),
  context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
  skills: [] as Array<{ name: string; scope: string }>,
}));

vi.mock('@/hooks/useTauriApi', () => ({ readSkillContent: fixtures.read }));
vi.mock('../workspace-context', () => ({
  useWorkspaceContextStore: { getState: () => ({ selectedContext: fixtures.context }) },
}));
vi.mock('../projects', () => ({ projectSnapshotFor: () => ({ projects: [] }) }));
vi.mock('@/lib/context', () => ({
  contextKey: () => 'native/global',
  globalContext: () => fixtures.context,
}));
vi.mock('@/lib/skills/identity', () => ({
  getSkillIdentity: (skill: { name: string }) => ({ name: skill.name }),
  getSkillIdentityKey: (identity: { name: string }) => identity.name,
  findSkillByIdentity: (identity: { name: string }) => (
    fixtures.skills.find((skill) => skill.name === identity.name) ?? null
  ),
}));
vi.mock('../skills-data', () => ({
  useSkillsDataStore: {
    getState: () => ({ snapshots: { 'native/global': { skills: fixtures.skills } } }),
  },
}));

import { useSkillDetailStore } from '../skill-detail';

function deferred() {
  let resolve!: (value: string) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<string>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

const a = { name: 'a', scope: 'global' } as InstalledSkill;
const b = { name: 'b', scope: 'global' } as InstalledSkill;

beforeEach(() => {
  useSkillDetailStore.getState().deselectSkill();
  fixtures.read.mockReset();
  fixtures.skills = [a, b];
});

describe('Skill detail request identity', () => {
  it('does not let an older A response win after A → B → A', async () => {
    const old = deferred();
    const latest = deferred();
    fixtures.read.mockReturnValueOnce(old.promise)
      .mockResolvedValueOnce('b')
      .mockReturnValueOnce(latest.promise);
    const first = useSkillDetailStore.getState().selectSkill(a);
    const middle = useSkillDetailStore.getState().selectSkill(b);
    const last = useSkillDetailStore.getState().selectSkill(a);
    latest.resolve('latest');
    await last;
    old.resolve('old');
    await Promise.all([first, middle]);
    expect(useSkillDetailStore.getState().skillContent).toBe('latest');
    expect(useSkillDetailStore.getState().loadingContent).toBe(false);
  });

  it('ignores an older failed reload after the newer reload succeeds', async () => {
    fixtures.read.mockResolvedValueOnce('initial');
    await useSkillDetailStore.getState().selectSkill(a);
    const old = deferred();
    const latest = deferred();
    fixtures.read.mockReturnValueOnce(old.promise).mockReturnValueOnce(latest.promise);
    const first = useSkillDetailStore.getState().reloadContent();
    const last = useSkillDetailStore.getState().reloadContent();
    await vi.waitFor(() => expect(fixtures.read).toHaveBeenCalledTimes(3));
    latest.resolve('latest');
    await last;
    old.reject(new Error('late failure'));
    await first;
    expect(useSkillDetailStore.getState().skillContent).toBe('latest');
  });

  it('invalidates the old request when the same Skill is selected after deselection', async () => {
    const old = deferred();
    fixtures.read.mockReturnValueOnce(old.promise).mockResolvedValueOnce('latest');
    const first = useSkillDetailStore.getState().selectSkill(a);
    useSkillDetailStore.getState().deselectSkill();
    await useSkillDetailStore.getState().selectSkill(a);
    old.resolve('old');
    await first;
    expect(useSkillDetailStore.getState().skillContent).toBe('latest');
  });

  it('preserves the same-selection no-op while a read is pending', async () => {
    const pending = deferred();
    fixtures.read.mockReturnValueOnce(pending.promise);
    const first = useSkillDetailStore.getState().selectSkill(a);
    await useSkillDetailStore.getState().selectSkill(a);
    expect(fixtures.read).toHaveBeenCalledTimes(1);
    pending.resolve('selected');
    await first;
    expect(useSkillDetailStore.getState().skillContent).toBe('selected');
  });
});
