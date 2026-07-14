// src/stores/__tests__/skills.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { ActiveMutation, ContextRef, SkillAgentDetails } from '@/bindings';
import { toast } from 'sonner';
import { useSkillsDataStore } from '../skills-data';
import { useSkillDetailStore } from '../skill-detail';
import { useSkillDialogStore } from '../skill-dialog';
import { useWorkspaceContextStore } from '../workspace-context';
import { useEnvironmentStore } from '../environment';
import { useProjectStore } from '../projects';
import { useMutationStore } from '../mutation';
import { buildUpdatePlan, clearUpdateCacheForSkill, mergeUpdateInfo, updateInfoCache, type SkillListItem } from '../skills-utils';
import { contextKey } from '@/lib/context';

const mockListSkills = vi.fn();
const mockListAgents = vi.fn();
const mockRemoveSkill = vi.fn();
const mockGetAgentDetails = vi.fn();
const mockCheckUpdates = vi.fn();
const mockUpdateSkill = vi.fn();
const mockUpdateSkillsBatch = vi.fn();
const mockOpenInstallWizard = vi.fn();
const mockCheckSkillAudit = vi.fn();
const mockReadSkillContent = vi.fn();
const mockManageSkillAgents = vi.fn();
const mockCleanupDuplicateAgentCopies = vi.fn();
const mockCopySkillToProjects = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mockListSkills(...args),
  listAgents: (...args: unknown[]) => mockListAgents(...args),
  removeSkill: (...args: unknown[]) => mockRemoveSkill(...args),
  getSkillAgentDetails: (...args: unknown[]) => mockGetAgentDetails(...args),
  checkUpdates: (...args: unknown[]) => mockCheckUpdates(...args),
  updateSkill: (...args: unknown[]) => mockUpdateSkill(...args),
  updateSkillsBatch: (...args: unknown[]) => mockUpdateSkillsBatch(...args),
  openInstallWizard: (...args: unknown[]) => mockOpenInstallWizard(...args),
  checkSkillAudit: (...args: unknown[]) => mockCheckSkillAudit(...args),
  readSkillContent: (...args: unknown[]) => mockReadSkillContent(...args),
  manageSkillAgents: (...args: unknown[]) => mockManageSkillAgents(...args),
  cleanupDuplicateAgentCopies: (...args: unknown[]) => mockCleanupDuplicateAgentCopies(...args),
  copySkillToProjects: (...args: unknown[]) => mockCopySkillToProjects(...args),
}));

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

vi.mock('@/utils/cross-storage-guidance', () => ({
  appendCrossStorageFailureGuidance: (
    message: string,
    _context: unknown,
    operation: string,
  ) => `${message}\nGUIDANCE:${operation}`,
  getCrossStorageFailureGuidance: (
    _context: unknown,
    operation: string,
  ) => `GUIDANCE:${operation}`,
}));

const makeSkill = (name: string, overrides: Partial<SkillListItem> = {}): SkillListItem => ({
  name,
  description: '',
  path: `/home/.agents/skills/${name}`,
  canonicalPath: `/home/.agents/.skills-cache/${name}`,
  scope: 'global',
  agents: ['claude-code'],
  source: `https://github.com/test/${name}`,
  hasUpdate: false,
  ...overrides,
});

const hostGlobal: ContextRef = {
  environment: { kind: 'host' },
  scope: { scope: 'global' },
};
const hostProjectA: ContextRef = {
  environment: { kind: 'host' },
  scope: { scope: 'project', project_id: 'project-a' },
};
const hostProjectB: ContextRef = {
  environment: { kind: 'host' },
  scope: { scope: 'project', project_id: 'project-b' },
};
function skillSnapshot(skills: SkillListItem[] = [], pathExists = true) {
  return {
    skills,
    agents: [],
    pathExists,
    loading: false,
    error: null,
    requestId: 1,
  };
}

const activeMutation: ActiveMutation = {
  kind: 'install',
  context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
  id: 'mutation-1',
  phase: 'preparing',
  progress: null,
  cancelable: true,
};

function setExplicitCrossStorageProjectContext() {
  const context = {
    environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
    scope: { scope: 'project' as const, project_id: 'project-1' },
  };
  useWorkspaceContextStore.setState({ selectedContext: context });
  useEnvironmentStore.setState({
    environments: [
      { environment: { kind: 'host' }, displayName: 'Windows', status: 'available' },
      {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        displayName: 'Ubuntu',
        status: 'available',
      },
    ],
  });
  useProjectStore.setState({
    projectsByEnvironment: {
      'wsl:Ubuntu': [{
        binding: {
          id: 'project-1',
          nativePath: '/mnt/c/Code/app',
          displayName: 'app',
          order: null,
          suppressCrossStorageWarning: false,
        },
        storage: { access: 'crossStorage', owner: { kind: 'host' } },
      }],
    },
  });
  return context;
}

const initialSkillsDataActions = {
  syncSkills: useSkillsDataStore.getState().syncSkills,
};

describe('useSkillsStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useWorkspaceContextStore.setState({ selectedContext: hostGlobal });
    useProjectStore.setState({
      projectsByEnvironment: {
        host: ['/project-a', '/project-b', '/a', '/b'].map((nativePath, index) => ({
          binding: {
            id: index < 2 ? `project-${index === 0 ? 'a' : 'b'}` : nativePath.slice(1),
            nativePath,
            displayName: null,
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: { access: 'native' as const, owner: { kind: 'host' as const } },
        })),
      },
      loadStateByEnvironment: {},
      errorsByEnvironment: {},
    });
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
    mockListSkills.mockResolvedValue({ skills: [], pathExists: true });
    mockListAgents.mockResolvedValue([]);
    mockCheckUpdates.mockResolvedValue([]);
    mockOpenInstallWizard.mockResolvedValue(undefined);
    updateInfoCache.clear();
    mockUpdateSkill.mockReset();
    mockUpdateSkillsBatch.mockReset();
    useSkillsDataStore.setState({
      snapshots: {},
      auditCache: {},
      isSyncing: false,
      checkingUpdateScopes: new Set(),
      updatingSkills: new Map(),
      lastUpdatePlan: null,
      lastUpdateResults: null,
      lastFailedUpdateNames: [],
      syncSkills: initialSkillsDataActions.syncSkills,
    });
    useSkillDetailStore.setState({
      selectedSkillRef: null,
      skillContent: null,
      loadingContent: false,
    });
    useSkillDialogStore.setState({
      deleteTarget: null,
      agentDetails: null,
      loadingAgentDetails: false,
      manageAgentsSkill: null,
      manageAgentsScope: 'global',
      manageAgentsProjectPath: undefined,
      manageAgentsContext: undefined,
      copySkill: null,
      copyContext: undefined,
      repairSourceTarget: null,
    });
  });

  it('blocks every skill write action while another mutation is active', async () => {
    const skill = makeSkill('toolkit', {
      hasUpdate: true,
      canRunUpdate: true,
      scope: 'global',
    });
    useMutationStore.setState({ activeMutation });
    useSkillsDataStore.setState({ snapshots: { [contextKey(hostGlobal)]: skillSnapshot([skill]) } });
    useSkillDialogStore.setState({
      deleteTarget: { skill, scope: 'global', context: hostGlobal },
      manageAgentsSkill: skill,
      manageAgentsScope: 'global',
      manageAgentsContext: hostGlobal,
      copySkill: skill,
      copyContext: hostGlobal,
    });

    useSkillDialogStore.getState().openAdd(hostGlobal);
    await useSkillDialogStore.getState().deleteSkill({ fullRemoval: true });
    await useSkillDialogStore.getState().saveAgentChanges(['cursor'], [], 'copy');
    await useSkillDialogStore.getState().cleanupDuplicateCopies(['cursor']);
    await useSkillDialogStore.getState().executeCopy(['/project']);
    await useSkillsDataStore.getState().updateSkill(hostGlobal, 'toolkit');
    await useSkillsDataStore.getState().updateAllInSection(hostGlobal);

    expect(mockOpenInstallWizard).not.toHaveBeenCalled();
    expect(mockRemoveSkill).not.toHaveBeenCalled();
    expect(mockManageSkillAgents).not.toHaveBeenCalled();
    expect(mockCleanupDuplicateAgentCopies).not.toHaveBeenCalled();
    expect(mockCopySkillToProjects).not.toHaveBeenCalled();
    expect(mockUpdateSkill).not.toHaveBeenCalled();
    expect(mockUpdateSkillsBatch).not.toHaveBeenCalled();
  });

  describe('mergeUpdateInfo', () => {
    it('preserves backend update reason when cached updates have no matching record', () => {
      const skills = [
        makeSkill('legacy', {
          canRunUpdate: false,
          canCheckForUpdates: false,
          updateReason: 'missing-skill-path',
        }),
      ];

      const merged = mergeUpdateInfo(skills, []);

      expect(merged[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        updateReason: 'missing-skill-path',
      }));
    });

    it('merges update info by source ref and skill path before name fallback', () => {
      const skills = [
        {
          ...makeSkill('demo', {
            scope: 'project',
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
            hasUpdate: false,
            canRunUpdate: true,
            canCheckForUpdates: true,
          }),
          skillPath: 'skills/demo/SKILL.md',
        },
      ];

      const merged = mergeUpdateInfo(skills, [
        {
          name: 'demo',
          source: 'owner/repo',
          sourceUrl: 'https://github.com/owner/repo',
          gitRef: 'main',
          skillPath: 'skills/demo/SKILL.md',
          hasUpdate: false,
          status: 'deleted-upstream',
          reason: 'deleted-upstream',
        },
      ]);

      expect(merged[0].updateStatus).toBe('deleted-upstream');
      expect(merged[0].updateReason).toBe('deleted-upstream');
    });
  });

  describe('forceCheckUpdates', () => {
    it('merges cannot-check status into skills instead of treating it as up to date', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([
            makeSkill('toolkit', { hasUpdate: false, canRunUpdate: true }),
          ]),
        },
      });
      mockCheckUpdates.mockResolvedValue([
        {
          name: 'toolkit',
          source: 'https://github.com/test/toolkit',
          hasUpdate: false,
          status: 'cannot-check',
          reason: 'missing-skill-path',
          gitRef: null,
        },
      ]);

      await useSkillsDataStore.getState().forceCheckUpdates(hostGlobal);

      expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]).toEqual(
        expect.objectContaining({
          hasUpdate: false,
          canRunUpdate: true,
          updateStatus: 'cannot-check',
          updateReason: 'missing-skill-path',
        })
      );
    });

    it('returns false and preserves cached results when update checking fails', async () => {
      updateInfoCache.set(contextKey(hostGlobal), {
        checkedAt: 1,
        results: [{ name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'update-available', gitRef: null }],
      });
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([makeSkill('toolkit', { hasUpdate: true })]),
        },
      });
      mockCheckUpdates.mockRejectedValue(new Error('network down'));

      const result = await useSkillsDataStore.getState().forceCheckUpdates(hostGlobal);

      expect(result).toBe(false);
      expect(updateInfoCache.get(contextKey(hostGlobal))).toEqual({
        checkedAt: 1,
        results: [{ name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'update-available', gitRef: null }],
      });
      expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]?.hasUpdate).toBe(true);
    });

    it('shows an error toast when a manual update check fails', async () => {
      mockCheckUpdates.mockRejectedValue(new Error('network down'));

      await useSkillsDataStore.getState().forceCheckUpdates(hostGlobal);

      expect(toast.error).toHaveBeenCalledTimes(1);
    });
  });

  describe('dialog state', () => {
    it('openAddWithPrefill opens a project-scoped install wizard when repair metadata includes project context', () => {
      useSkillDialogStore.getState().openAddWithPrefill({
        source: 'https://github.com/owner/repo#main',
        skillName: 'toolkit',
        scope: 'project',
        projectPath: 'D:\\Code\\project-a',
      }, hostProjectA);

      expect(mockOpenInstallWizard).toHaveBeenCalledWith({
        entryPoint: 'discovery',
        projectPath: 'D:\\Code\\project-a',
        prefillSource: 'https://github.com/owner/repo#main',
        prefillSkillName: 'toolkit',
        context: hostProjectA,
      });
    });

    it('openRepairSource stores a normalized repair target without opening the install wizard', () => {
      useSkillDialogStore.getState().openRepairSource(
        makeSkill('toolkit', {
          scope: 'project',
          source: 'owner/repo',
          sourceUrl: null,
          gitRef: 'main',
          agents: ['claude-code'],
        }),
        hostProjectA,
        'D:\\Code\\project-a'
      );

      expect(useSkillDialogStore.getState().repairSourceTarget).toEqual(expect.objectContaining({
        skillName: 'toolkit',
        scope: 'project',
        projectPath: 'D:\\Code\\project-a',
        source: 'https://github.com/owner/repo#main',
        agents: ['claude-code'],
      }));
      expect(mockOpenInstallWizard).not.toHaveBeenCalled();
    });

    it('openRepairSource ignores skills without a repairable source', () => {
      useSkillDialogStore.getState().openRepairSource(
        makeSkill('local-only', {
          source: 'not a url',
          sourceUrl: null,
          gitRef: null,
        }),
        hostGlobal,
      );

      expect(useSkillDialogStore.getState().repairSourceTarget).toBeNull();
      expect(mockOpenInstallWizard).not.toHaveBeenCalled();
    });

    it('openDelete sets deleteTarget and fetches agent details', async () => {
      const skill = makeSkill('test-skill');
      const details: SkillAgentDetails = {
        skillName: 'test-skill',
        scope: 'global',
        canonicalPath: '/tmp',
        automaticAgents: [],
        independentAgents: [],
        defaultAvailableAgents: [],
        privateRequiredAgents: [],
        duplicateCopyAgents: [],
        privateOnlyAgents: [],
      };
      mockGetAgentDetails.mockResolvedValue(details);

      useSkillDialogStore.getState().openDelete(skill, hostGlobal);

      expect(useSkillDialogStore.getState().deleteTarget).toBeTruthy();
      expect(useSkillDialogStore.getState().deleteTarget!.skill.name).toBe('test-skill');
      expect(useSkillDialogStore.getState().loadingAgentDetails).toBe(true);

      await vi.waitFor(() => {
        expect(useSkillDialogStore.getState().loadingAgentDetails).toBe(false);
      });
      expect(useSkillDialogStore.getState().agentDetails).toEqual(details);
    });

    it('closeDelete clears all delete state', () => {
      useSkillDialogStore.setState({
        deleteTarget: { skill: makeSkill('x'), scope: 'global', context: hostGlobal },
        agentDetails: {
          skillName: 'x',
          scope: 'global',
          canonicalPath: '/tmp',
          automaticAgents: [],
          independentAgents: [],
          defaultAvailableAgents: [],
          privateRequiredAgents: [],
          duplicateCopyAgents: [],
          privateOnlyAgents: [],
        } satisfies SkillAgentDetails,
        loadingAgentDetails: true,
      });

      useSkillDialogStore.getState().closeDelete();

      expect(useSkillDialogStore.getState().deleteTarget).toBeNull();
      expect(useSkillDialogStore.getState().agentDetails).toBeNull();
      expect(useSkillDialogStore.getState().loadingAgentDetails).toBe(false);
    });

    it('uses canonical details and remove commands for an explicit WSL context', async () => {
      const context = {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        scope: { scope: 'project', project_id: 'project-1' },
      } as const;
      const skill = makeSkill('toolkit', { scope: 'project' });
      mockGetAgentDetails.mockResolvedValue({
        skillName: 'toolkit',
        scope: 'project',
        canonicalPath: '/home/me/project/.agents/skills/toolkit',
        automaticAgents: [],
        independentAgents: [],
        defaultAvailableAgents: [],
        privateRequiredAgents: [],
        duplicateCopyAgents: [],
        privateOnlyAgents: [],
      });
      mockRemoveSkill.mockResolvedValue({ removed: true, removedPaths: [] });
      useWorkspaceContextStore.setState({ selectedContext: context });

      useSkillDialogStore.getState().openDelete(skill, context);
      await vi.waitFor(() => expect(mockGetAgentDetails).toHaveBeenCalledWith(context, 'toolkit'));
      useWorkspaceContextStore.setState({
        selectedContext: {
          environment: { kind: 'wsl', distro_name: 'Debian' },
          scope: { scope: 'project', project_id: 'project-2' },
        },
      });
      await useSkillDialogStore.getState().deleteSkill({ fullRemoval: true });

      expect(mockRemoveSkill).toHaveBeenCalledWith(context, {
        name: 'toolkit',
        fullRemoval: true,
        agents: undefined,
        agentTargets: undefined,
      });
    });

    it('adds storage-owner guidance when project deletion fails', async () => {
      const context = setExplicitCrossStorageProjectContext();
      const skill = makeSkill('toolkit', { scope: 'project' });
      mockRemoveSkill.mockRejectedValue(new Error('permission denied'));
      useSkillDialogStore.setState({
        deleteTarget: {
          skill,
          scope: 'project',
          projectPath: '/mnt/c/Code/app',
          context,
        },
      });

      await useSkillDialogStore.getState().deleteSkill({ fullRemoval: true });

      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('GUIDANCE:delete'));
    });
  });

  describe('updateSkill', () => {
    it('uses updateSkill for an explicit WSL context', async () => {
      const context = {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        scope: { scope: 'global' },
      } as const;
      mockUpdateSkill.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'success',
          warnings: [],
          agentResults: [],
        }],
        summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
      });
      await useSkillsDataStore.getState().updateSkill(context, 'toolkit');

      expect(mockUpdateSkill).toHaveBeenCalledWith(context, 'toolkit');
    });

    it('adds storage-owner guidance when a project update throws', async () => {
      const context = setExplicitCrossStorageProjectContext();
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(context)]: skillSnapshot([
            makeSkill('toolkit', { scope: 'project', hasUpdate: true }),
          ]),
        },
      });
      mockUpdateSkill.mockRejectedValue(new Error('permission denied'));

      await useSkillsDataStore.getState().updateSkill(context, 'toolkit');

      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('GUIDANCE:update'));
    });

    it('tracks updating state by scope and name identity', async () => {
      let resolveUpdate: ((value: unknown) => void) | undefined;
      mockUpdateSkill.mockImplementation(
        () =>
          new Promise((resolve) => {
            resolveUpdate = resolve;
          })
      );

      const updatePromise = useSkillsDataStore.getState().updateSkill(hostGlobal, 'toolkit');

      expect(useSkillsDataStore.getState().updatingSkills.get('global:toolkit')).toBe('updating');

      const finishUpdate = resolveUpdate;
      if (finishUpdate) {
        finishUpdate({
          results: [{
            name: 'toolkit',
            status: 'success',
            warnings: [],
            durationMs: 20,
            agentResults: [],
          }],
          summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
        });
      }

      await updatePromise;
    });

    it('shows partial + warning feedback using update response details', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([makeSkill('toolkit', { hasUpdate: true })]),
        },
      });

      mockUpdateSkill.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'partial',
          error: 'Some agents failed',
          warnings: ['Failed to write global lock: permission denied'],
          durationMs: 20,
          agentResults: [
            { agent: 'cursor', status: 'success', error: null, durationMs: 12 },
            { agent: 'windsurf', status: 'failed', error: 'permission denied', durationMs: 8 },
          ],
        }],
        summary: { total: 1, succeeded: 0, partial: 1, failed: 0, skipped: 0 },
      });

      // 提前 seed cache,模拟前一次 check_updates 的结果
      updateInfoCache.set(contextKey(hostGlobal), {
        results: [{
          name: 'toolkit',
          source: 'owner/repo',
          hasUpdate: true,
          status: 'update-available',
          reason: null,
          gitRef: 'main',
        }],
        checkedAt: Date.now(),
      });

      await useSkillsDataStore.getState().updateSkill(hostGlobal, 'toolkit');

      expect(mockUpdateSkill).toHaveBeenCalledWith(hostGlobal, 'toolkit');
      expect(toast.warning).toHaveBeenCalledTimes(2);
      expect(toast.success).not.toHaveBeenCalled();
      expect(toast.error).not.toHaveBeenCalled();

      // Fix 2 回归:partial 不应清缓存,失败 agent 信息应留在 UI
      expect(updateInfoCache.get(contextKey(hostGlobal))?.results[0]?.hasUpdate).toBe(true);
      expect(updateInfoCache.get(contextKey(hostGlobal))?.results[0]?.status).toBe('update-available');
      // 列表里的 hasUpdate 也应保留
      expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]?.hasUpdate).toBe(true);
    });

    it('refreshes selected skill content after a successful update while keeping identity selection', async () => {
      const updatedSkill = makeSkill('toolkit', {
        hasUpdate: false,
        updatedAt: '2026-04-07T12:00:00.000Z',
      });

      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([makeSkill('toolkit', { hasUpdate: true })]),
        },
      });
      useSkillDetailStore.setState({
        selectedSkillRef: { name: 'toolkit', scope: 'global', projectPath: null },
        selectedContext: hostGlobal,
        skillContent: '# Old content',
        loadingContent: false,
      });

      mockUpdateSkill.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'success',
          warnings: [],
          durationMs: 20,
          agentResults: [],
        }],
        summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
      });
      mockListSkills.mockResolvedValue({ skills: [updatedSkill], pathExists: true });
      mockReadSkillContent.mockResolvedValue('# New content');

      await useSkillsDataStore.getState().updateSkill(hostGlobal, 'toolkit');

      await vi.waitFor(() => {
        expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]?.hasUpdate).toBe(false);
        expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]?.updatedAt).toBe('2026-04-07T12:00:00.000Z');
        expect(useSkillDetailStore.getState().selectedSkillRef).toEqual({
          name: 'toolkit',
          scope: 'global',
          projectPath: null,
        });
        expect(useSkillDetailStore.getState().skillContent).toBe('# New content');
      });
    });

    it('clears the original project update cache even if context changes before completion', async () => {
      useWorkspaceContextStore.setState({ selectedContext: hostProjectA });
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostProjectA)]: skillSnapshot([
            makeSkill('toolkit', { scope: 'project', canCheckForUpdates: true }),
          ]),
        },
      });
      updateInfoCache.set(contextKey(hostProjectA), {
        checkedAt: Date.now(),
        results: [{ name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'update-available', gitRef: null }],
      });
      updateInfoCache.set(contextKey(hostProjectB), {
        checkedAt: Date.now(),
        results: [{ name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'update-available', gitRef: null }],
      });

      let resolveUpdate: ((value: unknown) => void) | undefined;
      mockUpdateSkill.mockImplementation(
        () =>
          new Promise((resolve) => {
            resolveUpdate = resolve;
          })
      );

      const updatePromise = useSkillsDataStore.getState().updateSkill(hostProjectA, 'toolkit');
      useWorkspaceContextStore.setState({ selectedContext: hostProjectB });

      const finishUpdate = resolveUpdate;
      if (finishUpdate) {
        finishUpdate({
          results: [{
            name: 'toolkit',
            status: 'success',
            warnings: [],
            durationMs: 20,
            agentResults: [],
          }],
          summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
        });
      }

      await updatePromise;

      expect(updateInfoCache.get(contextKey(hostProjectA))?.results[0]?.hasUpdate).toBe(false);
      expect(updateInfoCache.get(contextKey(hostProjectB))?.results[0]?.hasUpdate).toBe(true);
    });

    it('preserves cannot-check cache status when the skill cannot be checked for updates', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([
            makeSkill('toolkit', { canCheckForUpdates: false }),
          ]),
        },
      });
      updateInfoCache.set(contextKey(hostGlobal), {
        checkedAt: Date.now(),
        results: [{
          name: 'toolkit',
          source: 'owner/repo',
          hasUpdate: false,
          status: 'cannot-check',
          reason: 'missing-skill-path',
          gitRef: null,
        }],
      });

      mockUpdateSkill.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'success',
          warnings: [],
          durationMs: 20,
          agentResults: [],
        }],
        summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
      });

      await useSkillsDataStore.getState().updateSkill(hostGlobal, 'toolkit');

      const cached = updateInfoCache.get(contextKey(hostGlobal));
      expect(cached?.results[0]?.status).toBe('cannot-check');
      expect(cached?.results[0]?.reason).toBe('missing-skill-path');
    });

    it('preserves deleted-upstream cache status when clearing stale update flags', () => {
      updateInfoCache.set('global', {
        checkedAt: Date.now(),
        results: [{
          name: 'demo',
          source: 'owner/repo',
          hasUpdate: false,
          status: 'deleted-upstream',
          reason: 'deleted-upstream',
          gitRef: 'main',
          skillPath: 'skills/demo/SKILL.md',
        }],
      });

      clearUpdateCacheForSkill('demo', 'global', undefined, { clearCannotCheck: true });

      expect(updateInfoCache.get('global')?.results[0]).toEqual(expect.objectContaining({
        status: 'deleted-upstream',
        reason: 'deleted-upstream',
      }));
    });

    it('does not run ordinary update for upstream-deleted skills', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([{
            ...makeSkill('demo', {
              hasUpdate: false,
              canRunUpdate: true,
              updateReason: 'deleted-upstream',
            }),
            updateStatus: 'deleted-upstream',
          }]),
        },
      });

      await useSkillsDataStore.getState().updateSkill(hostGlobal, 'demo');

      expect(mockUpdateSkill).not.toHaveBeenCalled();
      expect(toast.info).toHaveBeenCalledWith('skills.updatePlan.deletedUpstreamDescription');
    });

    it('clears missing version metadata after a successful direct reinstall', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([{
            ...makeSkill('toolkit', {
              canCheckForUpdates: false,
              canRunUpdate: true,
              updateReason: 'missing-remote-hash',
            }),
            updateStatus: 'cannot-check',
          }]),
        },
      });
      updateInfoCache.set(contextKey(hostGlobal), {
        checkedAt: Date.now(),
        results: [{
          name: 'toolkit',
          source: 'owner/repo',
          hasUpdate: false,
          status: 'cannot-check',
          reason: 'missing-remote-hash',
          gitRef: null,
        }],
      });

      mockUpdateSkill.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'success',
          warnings: [],
          durationMs: 20,
          agentResults: [],
        }],
        summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
      });

      await useSkillsDataStore.getState().updateSkill(hostGlobal, 'toolkit');

      const cached = updateInfoCache.get(contextKey(hostGlobal));
      expect(cached?.results[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        status: 'up-to-date',
        reason: null,
      }));
      expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        updateStatus: 'up-to-date',
        updateReason: null,
      }));
    });

    it('clears missing source metadata after a successful source repair', () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([{
            ...makeSkill('toolkit', {
              canCheckForUpdates: false,
              canRunUpdate: false,
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannot-check',
          }]),
        },
      });
      updateInfoCache.set(contextKey(hostGlobal), {
        checkedAt: Date.now(),
        results: [{
          name: 'toolkit',
          source: 'owner/repo',
          hasUpdate: false,
          status: 'cannot-check',
          reason: 'missing-skill-path',
          gitRef: null,
        }],
      });

      useSkillsDataStore.getState().markSourceRepairSucceeded(hostGlobal, 'toolkit');

      const cached = updateInfoCache.get(contextKey(hostGlobal));
      expect(cached?.results[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        status: 'up-to-date',
        reason: null,
      }));
      expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        updateStatus: 'up-to-date',
        updateReason: null,
      }));
    });

    it('does not clear the visible project list when source repair completed for another project', () => {
      useWorkspaceContextStore.setState({ selectedContext: hostProjectB });
      const projectSkill = {
        ...makeSkill('toolkit', {
          scope: 'project' as const,
          canCheckForUpdates: false,
          canRunUpdate: false,
          updateReason: 'missing-skill-path',
        }),
        updateStatus: 'cannot-check' as const,
      };
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostProjectA)]: skillSnapshot([projectSkill]),
          [contextKey(hostProjectB)]: skillSnapshot([projectSkill]),
        },
      });
      updateInfoCache.set(contextKey(hostProjectA), {
        checkedAt: Date.now(),
        results: [{
          name: 'toolkit',
          source: 'owner/repo',
          hasUpdate: false,
          status: 'cannot-check',
          reason: 'missing-skill-path',
          gitRef: null,
        }],
      });

      useSkillsDataStore.getState().markSourceRepairSucceeded(hostProjectA, 'toolkit');

      const cached = updateInfoCache.get(contextKey(hostProjectA));
      expect(cached?.results[0]).toEqual(expect.objectContaining({
        status: 'up-to-date',
        reason: null,
      }));
      expect(useSkillsDataStore.getState().snapshots[contextKey(hostProjectB)].skills[0]).toEqual(expect.objectContaining({
        updateStatus: 'cannot-check',
        updateReason: 'missing-skill-path',
      }));
    });
  });

  describe('updateAllInSection', () => {
    it('builds an update plan with grouped updatable and repairable legacy skills', () => {
      const plan = buildUpdatePlan([
        makeSkill('toolkit', {
          hasUpdate: true,
          source: 'owner/repo',
          sourceUrl: 'https://github.com/owner/repo',
          gitRef: 'main',
        }),
        makeSkill('legacy', {
          hasUpdate: false,
          canRunUpdate: false,
          updateReason: 'missing-skill-path',
          source: 'owner/repo',
          sourceUrl: 'https://github.com/owner/repo',
          gitRef: 'main',
        }),
      ], 'project', 'D:\\Code\\project-a');

      expect(plan.total).toBe(2);
      expect(plan.updatableCount).toBe(1);
      expect(plan.repairableCount).toBe(1);
      expect(plan.groups).toHaveLength(1);
      expect(plan.groups[0]?.skillNames).toEqual(['toolkit']);
      expect(plan.groups[0]?.agents).toEqual(['claude-code']);
      expect(plan.repairable[0]).toEqual(expect.objectContaining({
        name: 'legacy',
        reason: 'missing-skill-path',
        repairSource: 'https://github.com/owner/repo#main',
      }));
    });

    it('puts deleted-upstream skills in a maintenance bucket instead of update groups', () => {
      const plan = buildUpdatePlan([
        {
          ...makeSkill('demo', {
            scope: 'project',
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
            hasUpdate: false,
            canRunUpdate: true,
            canCheckForUpdates: true,
            agents: ['claude-code'],
          }),
          skillPath: 'skills/demo/SKILL.md',
          updateStatus: 'deleted-upstream',
          updateReason: 'deleted-upstream',
        },
      ], 'project', '/repo');

      expect(plan.updatableCount).toBe(0);
      expect(plan.deletedUpstreamCount).toBe(1);
      expect(plan.deletedUpstream?.[0]).toEqual(expect.objectContaining({
        name: 'demo',
        reason: 'deleted-upstream',
        repairSource: 'https://github.com/owner/repo#main',
      }));
    });

    it('stores the last update plan and item results for update-all inspection', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([
            makeSkill('toolkit', { hasUpdate: true, canRunUpdate: true }),
          ]),
        },
      });
      mockListSkills.mockRejectedValue(new Error('sync failed'));
      mockUpdateSkillsBatch.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'success',
          warnings: [],
          durationMs: 20,
          agentResults: [],
        }],
        summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
      });

      await useSkillsDataStore.getState().updateAllInSection(hostGlobal);

      expect(useSkillsDataStore.getState().lastUpdatePlan?.updatableCount).toBe(1);
      expect(useSkillsDataStore.getState().lastUpdateResults?.[0]).toEqual(
        expect.objectContaining({ name: 'toolkit', status: 'success' })
      );
    });

    it('stores storage-owner guidance with failed project update results', async () => {
      const context = setExplicitCrossStorageProjectContext();
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(context)]: skillSnapshot([makeSkill('toolkit', {
            scope: 'project', hasUpdate: true, canRunUpdate: true,
          })]),
        },
      });
      mockUpdateSkillsBatch.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'failed',
          error: 'permission denied',
          warnings: [],
          agentResults: [],
        }],
        summary: { total: 1, succeeded: 0, partial: 0, failed: 1, skipped: 0 },
      });

      await useSkillsDataStore.getState().updateAllInSection(context);

      expect(useSkillsDataStore.getState().lastUpdateResults?.[0]?.error)
        .toContain('GUIDANCE:update');
    });

    it('records repairable legacy skills in the update plan without calling batch update', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([
          makeSkill('legacy', {
            hasUpdate: false,
            canRunUpdate: false,
            updateReason: 'missing-skill-path',
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
          }),
          ]),
        },
      });

      await useSkillsDataStore.getState().updateAllInSection(hostGlobal);

      expect(mockUpdateSkillsBatch).not.toHaveBeenCalled();
      expect(useSkillsDataStore.getState().lastUpdatePlan?.repairableCount).toBe(1);
      expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]?.updateReason)
        .toBe('missing-skill-path');
    });

    it('optimistically clears local hasUpdate flags after successful batch updates', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([makeSkill('toolkit', { hasUpdate: true })]),
        },
      });
      mockUpdateSkill.mockReset();
      mockListSkills.mockRejectedValue(new Error('sync failed'));
      mockUpdateSkillsBatch.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'success',
          warnings: [],
          durationMs: 20,
          agentResults: [],
        }],
        summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
      });

      await useSkillsDataStore.getState().updateAllInSection(hostGlobal);

      expect(useSkillsDataStore.getState().snapshots[contextKey(hostGlobal)].skills[0]?.hasUpdate).toBe(false);
    });

    it('sends one batch request when update candidates use different refs', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([
          makeSkill('toolkit-main', {
            hasUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
          makeSkill('toolkit-dev', {
            hasUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'dev',
          }),
          ]),
        },
      });
      mockListSkills.mockRejectedValue(new Error('sync failed'));
      mockUpdateSkillsBatch.mockImplementation(async (_context: ContextRef, names: string[]) => ({
        results: names.map((name) => ({
          name,
          status: 'success',
          warnings: [],
          durationMs: 20,
          agentResults: [],
        })),
        summary: {
          total: names.length,
          succeeded: names.length,
          partial: 0,
          failed: 0,
          skipped: 0,
        },
      }));

      await useSkillsDataStore.getState().updateAllInSection(hostGlobal);

      expect(mockUpdateSkillsBatch).toHaveBeenCalledTimes(1);
      expect(mockUpdateSkillsBatch).toHaveBeenCalledWith(
        hostGlobal,
        ['toolkit-main', 'toolkit-dev'],
      );
      expect(useSkillsDataStore.getState().lastUpdateResults).toHaveLength(2);
    });

    it('marks every requested skill failed when the single batch request rejects', async () => {
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([
            makeSkill('toolkit-main', {
              hasUpdate: true,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              gitRef: 'main',
            }),
            makeSkill('toolkit-dev', {
              hasUpdate: true,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              gitRef: 'dev',
            }),
          ]),
        },
      });
      mockListSkills.mockRejectedValue(new Error('sync failed'));
      mockUpdateSkillsBatch.mockRejectedValue(new Error('batch failed'));

      await useSkillsDataStore.getState().updateAllInSection(hostGlobal);

      expect(mockUpdateSkillsBatch).toHaveBeenCalledTimes(1);
      expect(useSkillsDataStore.getState().lastUpdateResults?.map((item) => item.status))
        .toEqual(['failed', 'failed']);
    });
  });

  describe('selectSkill / deselectSkill', () => {
    it('selectSkill sets selectedSkillRef and loads content', async () => {
      const skill = makeSkill('commit');
      mockReadSkillContent.mockResolvedValue('# Commit\n\nBody content');

      await useSkillDetailStore.getState().selectSkill(skill);

      const state = useSkillDetailStore.getState();
      expect(state.selectedSkillRef).toEqual({
        name: 'commit',
        scope: 'global',
        projectPath: null,
      });
      expect(state.skillContent).toBe('# Commit\n\nBody content');
      expect(state.loadingContent).toBe(false);
    });

    it('selectSkill ignores stale response on fast switch', async () => {
      const skill1 = makeSkill('skill-1');
      const skill2 = makeSkill('skill-2');

      mockReadSkillContent
        .mockImplementationOnce(() => new Promise((resolve) => setTimeout(() => resolve('content-1'), 100)))
        .mockImplementationOnce(() => Promise.resolve('content-2'));

      const p1 = useSkillDetailStore.getState().selectSkill(skill1);
      const p2 = useSkillDetailStore.getState().selectSkill(skill2);
      await Promise.all([p1, p2]);

      const state = useSkillDetailStore.getState();
      expect(state.selectedSkillRef?.name).toBe('skill-2');
      expect(state.skillContent).toBe('content-2');
    });

    it('selectSkill same skill is no-op', async () => {
      const skill = makeSkill('commit');
      mockReadSkillContent.mockResolvedValue('content');

      await useSkillDetailStore.getState().selectSkill(skill);
      mockReadSkillContent.mockClear();

      await useSkillDetailStore.getState().selectSkill(skill);
      expect(mockReadSkillContent).not.toHaveBeenCalled();
    });

    it('deselectSkill clears selection', async () => {
      const skill = makeSkill('commit');
      mockReadSkillContent.mockResolvedValue('content');
      await useSkillDetailStore.getState().selectSkill(skill);

      useSkillDetailStore.getState().deselectSkill();

      const state = useSkillDetailStore.getState();
      expect(state.selectedSkillRef).toBeNull();
      expect(state.skillContent).toBeNull();
      expect(state.loadingContent).toBe(false);
    });

    it('selectSkill handles load error gracefully', async () => {
      const skill = makeSkill('broken');
      mockReadSkillContent.mockRejectedValue(new Error('read failed'));

      await useSkillDetailStore.getState().selectSkill(skill);

      const state = useSkillDetailStore.getState();
      expect(state.selectedSkillRef).toEqual({
        name: 'broken',
        scope: 'global',
        projectPath: null,
      });
      expect(state.skillContent).toBeNull();
      expect(state.loadingContent).toBe(false);
    });

    it('reloadContent resolves the latest selected skill from skills-data', async () => {
      useSkillDetailStore.setState({
        selectedSkillRef: { name: 'commit', scope: 'global', projectPath: null },
        selectedContext: hostGlobal,
        skillContent: '# Old content',
        loadingContent: false,
      });
      useSkillsDataStore.setState({
        snapshots: {
          [contextKey(hostGlobal)]: skillSnapshot([
            makeSkill('commit', { canonicalPath: '/fresh/commit' }),
          ]),
        },
      });
      mockReadSkillContent.mockResolvedValue('# Fresh content');

      await useSkillDetailStore.getState().reloadContent();

      expect(mockReadSkillContent).toHaveBeenCalledWith(hostGlobal, '/fresh/commit');
      expect(useSkillDetailStore.getState().skillContent).toBe('# Fresh content');
    });
  });

  describe('manageAgents', () => {
    it('openManageAgents sets target skill and scope', () => {
      const skill = makeSkill('test', { scope: 'project' });
      useWorkspaceContextStore.setState({ selectedContext: hostProjectA });
      useSkillDialogStore.getState().openManageAgents(skill, hostProjectA);
      expect(useSkillDialogStore.getState().manageAgentsSkill).toBe(skill);
      expect(useSkillDialogStore.getState().manageAgentsScope).toBe('project');
      expect(useSkillDialogStore.getState().manageAgentsProjectPath).toBe('/project-a');
    });

    it('closeManageAgents clears target', () => {
      const skill = makeSkill('test');
      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsProjectPath: '/project-a' });
      useSkillDialogStore.getState().closeManageAgents();
      expect(useSkillDialogStore.getState().manageAgentsSkill).toBeNull();
      expect(useSkillDialogStore.getState().manageAgentsProjectPath).toBeUndefined();
    });

    it('saveAgentChanges calls API and syncs skills', async () => {
      const skill = makeSkill('test');
      mockManageSkillAgents.mockResolvedValue({ added: ['cursor'], addedResults: [], removed: [], errors: [] });
      mockListSkills.mockResolvedValue({ skills: [makeSkill('test', { agents: ['claude-code', 'cursor'] })], pathExists: true });
      mockListAgents.mockResolvedValue([]);

      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global', manageAgentsContext: hostGlobal });
      await useSkillDialogStore.getState().saveAgentChanges(['cursor'], [], 'copy');

      expect(mockManageSkillAgents).toHaveBeenCalledWith(hostGlobal, {
        skillName: 'test',
        addAgents: ['cursor'],
        removeAgents: [],
        privateCopyAgents: [],
        mode: 'copy',
      });
      expect(useSkillDialogStore.getState().manageAgentsSkill).toBeNull();
      expect(toast.success).toHaveBeenCalled();
    });

    it('saveAgentChanges passes private copy agents to the API', async () => {
      const skill = makeSkill('test');
      mockManageSkillAgents.mockResolvedValue({ added: [], addedResults: [], removed: [], errors: [] });

      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global', manageAgentsContext: hostGlobal });
      await useSkillDialogStore.getState().saveAgentChanges([], [], 'copy', ['firebender']);

      expect(mockManageSkillAgents).toHaveBeenCalledWith(hostGlobal, {
        skillName: 'test',
        addAgents: [],
        removeAgents: [],
        privateCopyAgents: ['firebender'],
        mode: 'copy',
      });
    });

    it('saveAgentChanges cleans deselected duplicate private copies before saving', async () => {
      const skill = makeSkill('test');
      mockCleanupDuplicateAgentCopies.mockResolvedValue([
        { agent: 'firebender', success: true, skipped: false, path: '/home/.firebender/skills/test', error: null },
      ]);
      mockManageSkillAgents.mockResolvedValue({ added: [], addedResults: [], removed: [], errors: [] });

      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global', manageAgentsContext: hostGlobal });
      await useSkillDialogStore.getState().saveAgentChanges([], [], 'copy', [], ['firebender']);

      expect(mockCleanupDuplicateAgentCopies).toHaveBeenCalledWith(hostGlobal, {
        skillName: 'test',
        agents: ['firebender'],
      });
      expect(mockManageSkillAgents).toHaveBeenCalledWith(hostGlobal, {
        skillName: 'test',
        addAgents: [],
        removeAgents: [],
        privateCopyAgents: [],
        mode: 'copy',
      });
    });

    it('saveAgentChanges keeps the project path from when the dialog was opened', async () => {
      const skill = makeSkill('test', { scope: 'project' });
      useWorkspaceContextStore.setState({ selectedContext: hostProjectA });
      mockManageSkillAgents.mockResolvedValue({ added: [], addedResults: [], removed: [], errors: [] });
      mockListSkills.mockResolvedValue({ skills: [], pathExists: true });

      useSkillDialogStore.getState().openManageAgents(skill, hostProjectA);
      useWorkspaceContextStore.setState({ selectedContext: hostProjectB });

      await useSkillDialogStore.getState().saveAgentChanges(['cursor'], [], 'symlink');

      expect(mockManageSkillAgents).toHaveBeenCalledWith(hostProjectA, {
        skillName: 'test',
        addAgents: ['cursor'],
        removeAgents: [],
        privateCopyAgents: [],
        mode: 'symlink',
      });
    });

    it('saveAgentChanges shows error toast on API errors', async () => {
      const skill = makeSkill('test');
      mockManageSkillAgents.mockResolvedValue({ added: [], addedResults: [], removed: [], errors: ['cursor: failed'] });
      mockListSkills.mockResolvedValue({ skills: [], pathExists: true });

      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global', manageAgentsContext: hostGlobal });
      await useSkillDialogStore.getState().saveAgentChanges(['cursor'], [], 'symlink');

      expect(toast.error).toHaveBeenCalled();
    });

    it('adds storage-owner guidance to project Agent management failures', async () => {
      const context = setExplicitCrossStorageProjectContext();
      const skill = makeSkill('test', { scope: 'project' });
      mockManageSkillAgents.mockResolvedValue({
        added: [],
        addedResults: [],
        removed: [],
        errors: ['cursor: permission denied'],
      });
      useSkillDialogStore.setState({
        manageAgentsSkill: skill,
        manageAgentsScope: 'project',
        manageAgentsProjectPath: '/mnt/c/Code/app',
        manageAgentsContext: context,
      });

      await useSkillDialogStore.getState().saveAgentChanges(['cursor'], [], 'copy');

      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('GUIDANCE:manageAgents'));
    });

    it('cleanupDuplicateCopies removes duplicate copies, refreshes details, and syncs skills', async () => {
      const skill = makeSkill('test');
      const refreshedDetails: SkillAgentDetails = {
        skillName: 'test',
        scope: 'global',
        canonicalPath: '/home/.agents/skills/test',
        defaultAvailableAgents: [{
          agent: 'claude-code',
          displayName: 'Claude Code',
          presence: 'default-active',
          sharedPath: '/home/.agents/skills/test',
          privatePath: null,
          canCleanupPrivateCopy: false,
        }],
        privateRequiredAgents: [],
        duplicateCopyAgents: [],
        privateOnlyAgents: [],
        automaticAgents: [['claude-code', 'Claude Code']],
        independentAgents: [],
      };
      const syncSkills = vi.fn().mockResolvedValue(undefined);
      mockCleanupDuplicateAgentCopies.mockResolvedValue([
        { agent: 'firebender', success: true, skipped: false, path: '/home/.firebender/skills/test', error: null },
      ]);
      mockGetAgentDetails.mockResolvedValue(refreshedDetails);
      useSkillsDataStore.setState({ syncSkills });

      useSkillDialogStore.setState({
        manageAgentsSkill: skill,
        manageAgentsScope: 'global',
        manageAgentsContext: hostGlobal,
        manageAgentDetails: {
          ...refreshedDetails,
          duplicateCopyAgents: [{
            agent: 'firebender',
            displayName: 'Firebender',
            presence: 'duplicate-copy',
            sharedPath: '/home/.agents/skills/test',
            privatePath: '/home/.firebender/skills/test',
            canCleanupPrivateCopy: true,
          }],
        },
      });
      await useSkillDialogStore.getState().cleanupDuplicateCopies(['firebender']);

      expect(mockCleanupDuplicateAgentCopies).toHaveBeenCalledWith(hostGlobal, {
        skillName: 'test',
        agents: ['firebender'],
      });
      expect(mockGetAgentDetails).toHaveBeenCalledWith(hostGlobal, 'test');
      expect(syncSkills).toHaveBeenCalledWith(hostGlobal);
      expect(useSkillDialogStore.getState().manageAgentDetails).toEqual(refreshedDetails);
      expect(toast.success).toHaveBeenCalled();
    });
  });

  describe('copyToProject', () => {
    it('openCopyToProject sets target skill', () => {
      const skill = makeSkill('test', { scope: 'project' });
      useSkillDialogStore.getState().openCopyToProject(skill, hostProjectA);
      expect(useSkillDialogStore.getState().copySkill).toBe(skill);
    });

    it('closeCopyToProject clears target', () => {
      useSkillDialogStore.setState({ copySkill: makeSkill('test') });
      useSkillDialogStore.getState().closeCopyToProject();
      expect(useSkillDialogStore.getState().copySkill).toBeNull();
    });

    it('executeCopy calls API and shows success toast', async () => {
      const skill = makeSkill('test', {
        scope: 'project',
        agents: ['antigravity', 'claude-code', 'firebender'],
        defaultAvailableAgents: ['antigravity'],
        privateAdaptedAgents: ['claude-code'],
        privateCopyAgents: ['firebender'],
      });
      mockCopySkillToProjects.mockResolvedValue({
        results: [{
          projectPath: '/project-b',
          success: true,
          error: null,
          defaultAvailableAgents: ['antigravity'],
          privateAdaptedAgents: ['claude-code'],
          privateCopyAgents: ['firebender'],
          skippedAgents: [],
          updateMetadataStatus: 'preserved',
          updateMetadataReason: null,
        }],
      });

      useSkillDialogStore.setState({ copySkill: skill, copyContext: hostProjectA });
      await useSkillDialogStore.getState().executeCopy(['/project-b']);

      expect(mockCopySkillToProjects).toHaveBeenCalledWith({
        skillName: 'test',
        source: hostProjectA,
        targets: [hostProjectB],
        agents: ['claude-code'],
        privateCopyAgents: ['firebender'],
      });
      expect(useSkillDialogStore.getState().copySkill).toBeNull();
      expect(toast.success).toHaveBeenCalled();
    });

    it('executeCopy warns when target agents are skipped', async () => {
      const skill = makeSkill('test', {
        scope: 'project',
        agents: ['claude-code'],
        privateAdaptedAgents: ['claude-code'],
      });
      mockCopySkillToProjects.mockResolvedValue({
        results: [{
          projectPath: '/project-b',
          success: true,
          error: null,
          defaultAvailableAgents: ['antigravity'],
          privateAdaptedAgents: [],
          privateCopyAgents: [],
          skippedAgents: ['claude-code'],
          updateMetadataStatus: 'preserved',
          updateMetadataReason: null,
        }],
      });

      useSkillDialogStore.setState({ copySkill: skill, copyContext: hostProjectA });
      await useSkillDialogStore.getState().executeCopy(['/project-b']);

      expect(toast.warning).toHaveBeenCalledWith('skills.copyToProject.skippedAgents');
    });

    it('executeCopy shows error toast on partial failure', async () => {
      const skill = makeSkill('test', { scope: 'project' });
      mockCopySkillToProjects.mockResolvedValue({
        results: [
          {
            projectPath: '/a',
            success: true,
            error: null,
            updateMetadataStatus: 'preserved',
            updateMetadataReason: null,
          },
          {
            projectPath: '/b',
            success: false,
            error: 'disk full',
            updateMetadataStatus: 'missing',
            updateMetadataReason: 'copy-failed',
          },
        ],
      });

      useSkillDialogStore.setState({ copySkill: skill, copyContext: hostProjectA });
      await useSkillDialogStore.getState().executeCopy(['/a', '/b']);

      expect(toast.error).toHaveBeenCalled();
    });

    it('adds storage-owner guidance to cross-storage copy failures', async () => {
      const context = setExplicitCrossStorageProjectContext();
      const skill = makeSkill('test', { scope: 'project' });
      mockCopySkillToProjects.mockResolvedValue({
        results: [{
          projectPath: '/mnt/c/Code/app',
          success: false,
          error: 'permission denied',
          updateMetadataStatus: 'missing',
          updateMetadataReason: 'copy-failed',
        }],
      });
      useSkillDialogStore.setState({ copySkill: skill, copyContext: context });

      await useSkillDialogStore.getState().executeCopy(['/mnt/c/Code/app']);

      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('GUIDANCE:copy'));
    });

    it('executeCopy shows a normal success toast when copied projects cannot keep update metadata', async () => {
      const skill = makeSkill('test', { scope: 'project' });
      mockCopySkillToProjects.mockResolvedValue({
        results: [{
          projectPath: '/project-b',
          success: true,
          error: null,
          defaultAvailableAgents: ['antigravity'],
          privateAdaptedAgents: [],
          privateCopyAgents: [],
          skippedAgents: [],
          updateMetadataStatus: 'incomplete',
          updateMetadataReason: 'missing-remote-hash',
        }],
      });

      useSkillDialogStore.setState({ copySkill: skill, copyContext: hostProjectA });
      await useSkillDialogStore.getState().executeCopy(['/project-b']);

      expect(toast.success).toHaveBeenCalledWith('skills.copyToProject.success');
      expect(toast.warning).not.toHaveBeenCalledWith('skills.copyToProject.metadataIncomplete');
    });

    it('executeCopy reports copy failures without adding metadata warnings', async () => {
      const skill = makeSkill('test', { scope: 'project' });
      mockCopySkillToProjects.mockResolvedValue({
        results: [
          {
            projectPath: '/a',
            success: true,
            error: null,
            defaultAvailableAgents: [],
            privateAdaptedAgents: [],
            privateCopyAgents: [],
            skippedAgents: [],
            updateMetadataStatus: 'missing',
            updateMetadataReason: 'missing-source',
          },
          {
            projectPath: '/b',
            success: false,
            error: 'disk full',
            defaultAvailableAgents: [],
            privateAdaptedAgents: [],
            privateCopyAgents: [],
            skippedAgents: [],
            updateMetadataStatus: 'missing',
            updateMetadataReason: 'copy-failed',
          },
        ],
      });

      useSkillDialogStore.setState({ copySkill: skill, copyContext: hostProjectA });
      await useSkillDialogStore.getState().executeCopy(['/a', '/b']);

      expect(toast.error).toHaveBeenCalled();
      expect(toast.warning).not.toHaveBeenCalledWith('skills.copyToProject.metadataIncomplete');
    });
  });
});
