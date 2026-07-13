// src/stores/__tests__/skills.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { InstalledSkill, SkillAgentDetails } from '@/bindings';
import { toast } from 'sonner';
import { useSkillsDataStore } from '../skills-data';
import { useSkillDetailStore } from '../skill-detail';
import { useSkillDialogStore } from '../skill-dialog';
import { useContextStore } from '../context';
import { useEnvironmentStore } from '../environment';
import { buildUpdatePlan, clearUpdateCacheForSkill, mergeUpdateInfo, updateInfoCache } from '../skills-utils';

const mockListSkills = vi.fn();
const mockListSkillsV2 = vi.fn();
const mockListAgents = vi.fn();
const mockListAgentsForProject = vi.fn();
const mockListAgentsForProjectV2 = vi.fn();
const mockRemoveSkill = vi.fn();
const mockRemoveSkillV2 = vi.fn();
const mockGetAgentDetails = vi.fn();
const mockGetAgentDetailsV2 = vi.fn();
const mockCheckUpdates = vi.fn();
const mockCheckUpdatesV2 = vi.fn();
const mockUpdateSkill = vi.fn();
const mockUpdateSkillV2 = vi.fn();
const mockUpdateSkillsBatch = vi.fn();
const mockUpdateSkillsBatchV2 = vi.fn();
const mockOpenInstallWizard = vi.fn();
const mockCheckSkillAudit = vi.fn();
const mockReadSkillContent = vi.fn();
const mockManageSkillAgents = vi.fn();
const mockManageSkillAgentsV2 = vi.fn();
const mockCleanupDuplicateAgentCopies = vi.fn();
const mockCleanupDuplicateAgentCopiesV2 = vi.fn();
const mockCopySkillToProjects = vi.fn();
const mockCopySkillToProjectsV2 = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mockListSkills(...args),
  listSkillsV2: (...args: unknown[]) => mockListSkillsV2(...args),
  listAgents: (...args: unknown[]) => mockListAgents(...args),
  listAgentsForProject: (...args: unknown[]) => mockListAgentsForProject(...args),
  listAgentsForProjectV2: (...args: unknown[]) => mockListAgentsForProjectV2(...args),
  removeSkill: (...args: unknown[]) => mockRemoveSkill(...args),
  removeSkillV2: (...args: unknown[]) => mockRemoveSkillV2(...args),
  getSkillAgentDetails: (...args: unknown[]) => mockGetAgentDetails(...args),
  getSkillAgentDetailsV2: (...args: unknown[]) => mockGetAgentDetailsV2(...args),
  checkUpdates: (...args: unknown[]) => mockCheckUpdates(...args),
  checkUpdatesV2: (...args: unknown[]) => mockCheckUpdatesV2(...args),
  updateSkill: (...args: unknown[]) => mockUpdateSkill(...args),
  updateSkillV2: (...args: unknown[]) => mockUpdateSkillV2(...args),
  updateSkillsBatch: (...args: unknown[]) => mockUpdateSkillsBatch(...args),
  updateSkillsBatchV2: (...args: unknown[]) => mockUpdateSkillsBatchV2(...args),
  openInstallWizard: (...args: unknown[]) => mockOpenInstallWizard(...args),
  checkSkillAudit: (...args: unknown[]) => mockCheckSkillAudit(...args),
  readSkillContent: (...args: unknown[]) => mockReadSkillContent(...args),
  manageSkillAgents: (...args: unknown[]) => mockManageSkillAgents(...args),
  manageSkillAgentsV2: (...args: unknown[]) => mockManageSkillAgentsV2(...args),
  cleanupDuplicateAgentCopies: (...args: unknown[]) => mockCleanupDuplicateAgentCopies(...args),
  cleanupDuplicateAgentCopiesV2: (...args: unknown[]) => mockCleanupDuplicateAgentCopiesV2(...args),
  copySkillToProjects: (...args: unknown[]) => mockCopySkillToProjects(...args),
  copySkillToProjectsV2: (...args: unknown[]) => mockCopySkillToProjectsV2(...args),
}));

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

const makeSkill = (name: string, overrides: Partial<InstalledSkill> = {}): InstalledSkill => ({
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

const initialSkillsDataActions = {
  syncSkills: useSkillsDataStore.getState().syncSkills,
};

describe('useSkillsStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useContextStore.setState({
      selectedContext: 'global',
      hasExplicitContext: false,
      selectedContextRef: { environment: { kind: 'host' }, scope: { scope: 'global' } },
    });
    useEnvironmentStore.setState({
      selectedEnvironment: { kind: 'host' },
      projectsByEnvironment: {},
      projectsLoaded: {},
    });
    mockListSkills.mockResolvedValue({ skills: [], pathExists: true });
    mockListSkillsV2.mockResolvedValue({ skills: [], pathExists: true });
    mockListAgents.mockResolvedValue([]);
    mockListAgentsForProject.mockResolvedValue([]);
    mockListAgentsForProjectV2.mockResolvedValue([]);
    mockCheckUpdates.mockResolvedValue([]);
    mockCheckUpdatesV2.mockResolvedValue([]);
    mockOpenInstallWizard.mockResolvedValue(undefined);
    updateInfoCache.clear();
    mockUpdateSkillsBatch.mockReset();
    useSkillsDataStore.setState({
      globalSkills: [],
      projectSkills: [],
      projectPathExists: true,
      allAgents: [],
      loading: true,
      error: null,
      auditCache: {},
      allProjectsSkills: new Map(),
      isSyncing: false,
      checkingUpdateScopes: new Set(),
      updatingSkills: new Map(),
      updateAllCancelled: false,
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

  it('loads Discover project locations from the explicit WSL environment', async () => {
    const context = {
      environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      scope: { scope: 'global' as const },
    };
    useContextStore.setState({
      selectedContext: 'global',
      selectedContextRef: context,
      hasExplicitContext: true,
    });
    useEnvironmentStore.setState({
      selectedEnvironment: context.environment,
      projectsByEnvironment: {
        'wsl:Ubuntu': [{
          id: 'project-1',
          nativePath: '/home/me/app',
          displayName: null,
          order: null,
          suppressCrossStorageWarning: false,
        }],
      },
      projectsLoaded: { 'wsl:Ubuntu': true },
    });
    mockListSkillsV2.mockResolvedValue({
      skills: [makeSkill('toolkit', { scope: 'project' })],
      pathExists: true,
    });

    await useSkillsDataStore.getState().fetchAllProjectsSkills();

    expect(mockListSkillsV2).toHaveBeenCalledWith({
      environment: context.environment,
      scope: { scope: 'project', project_id: 'project-1' },
    });
    expect(useSkillsDataStore.getState().allProjectsSkills.get('/home/me/app'))
      .toEqual([expect.objectContaining({ name: 'toolkit' })]);
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

  describe('fetchSkills — global scope', () => {
    it('loads global skills when context is global', async () => {
      const skills = [makeSkill('toolkit'), makeSkill('analyzer')];
      mockListAgents.mockResolvedValue([]);
      mockListSkills.mockResolvedValue({ skills, pathExists: true });

      await useSkillsDataStore.getState().fetchSkills();

      const state = useSkillsDataStore.getState();
      expect(state.globalSkills).toHaveLength(2);
      expect(state.globalSkills[0].name).toBe('analyzer');
      expect(state.globalSkills[1].name).toBe('toolkit');
      expect(state.projectSkills).toEqual([]);
      expect(state.loading).toBe(false);
      expect(state.error).toBeNull();
    });
  });

  describe('fetchSkills — project scope', () => {
    it('loads both global and project skills when project is selected', async () => {
      useContextStore.setState({ selectedContext: '/my/project' });
      mockListAgentsForProject.mockResolvedValue([]);
      mockListSkills
        .mockResolvedValueOnce({ skills: [makeSkill('global-skill')], pathExists: true })
        .mockResolvedValueOnce({ skills: [makeSkill('project-skill')], pathExists: true });

      await useSkillsDataStore.getState().fetchSkills();

      const state = useSkillsDataStore.getState();
      expect(state.globalSkills).toHaveLength(1);
      expect(state.projectSkills).toHaveLength(1);
      expect(mockListAgentsForProject).toHaveBeenCalledWith('/my/project');
      expect(mockListAgents).not.toHaveBeenCalled();
    });
  });

  describe('fetchSkills — error handling', () => {
    it('sets error state on failure', async () => {
      mockListAgents.mockRejectedValue(new Error('network down'));

      await useSkillsDataStore.getState().fetchSkills();

      expect(useSkillsDataStore.getState().error).toBe('network down');
      expect(useSkillsDataStore.getState().loading).toBe(false);
    });
  });

  describe('forceCheckUpdates', () => {
    it('merges cannot-check status into skills instead of treating it as up to date', async () => {
      useSkillsDataStore.setState({
        globalSkills: [makeSkill('toolkit', { hasUpdate: false, canRunUpdate: true })],
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

      await useSkillsDataStore.getState().forceCheckUpdates('global');

      expect(useSkillsDataStore.getState().globalSkills[0]).toEqual(
        expect.objectContaining({
          hasUpdate: false,
          canRunUpdate: true,
          updateStatus: 'cannot-check',
          updateReason: 'missing-skill-path',
        })
      );
    });

    it('returns false and preserves cached results when update checking fails', async () => {
      updateInfoCache.set('global', {
        checkedAt: 1,
        results: [{ name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'update-available', gitRef: null }],
      });
      useSkillsDataStore.setState({
        globalSkills: [makeSkill('toolkit', { hasUpdate: true })],
      });
      mockCheckUpdates.mockRejectedValue(new Error('network down'));

      const result = await useSkillsDataStore.getState().forceCheckUpdates('global');

      expect(result).toBe(false);
      expect(updateInfoCache.get('global')).toEqual({
        checkedAt: 1,
        results: [{ name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'update-available', gitRef: null }],
      });
      expect(useSkillsDataStore.getState().globalSkills[0]?.hasUpdate).toBe(true);
    });

    it('shows an error toast when a manual update check fails', async () => {
      mockCheckUpdates.mockRejectedValue(new Error('network down'));

      await useSkillsDataStore.getState().forceCheckUpdates('global');

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
      });

      expect(mockOpenInstallWizard).toHaveBeenCalledWith({
        entryPoint: 'discovery',
        scope: 'project',
        projectPath: 'D:\\Code\\project-a',
        prefillSource: 'https://github.com/owner/repo#main',
        prefillSkillName: 'toolkit',
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
        'project',
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
        'global'
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

      useSkillDialogStore.getState().openDelete(skill, 'global');

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
        deleteTarget: { skill: makeSkill('x'), scope: 'global' },
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

    it('uses v2 details and remove commands for an explicit WSL context', async () => {
      const context = {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        scope: { scope: 'project', project_id: 'project-1' },
      } as const;
      const skill = makeSkill('toolkit', { scope: 'project' });
      mockGetAgentDetailsV2.mockResolvedValue({
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
      mockRemoveSkillV2.mockResolvedValue({ removed: true, removedPaths: [] });
      useContextStore.setState({
        selectedContextRef: context,
        hasExplicitContext: true,
      });

      useSkillDialogStore.getState().openDelete(skill, 'project');
      await vi.waitFor(() => expect(mockGetAgentDetailsV2).toHaveBeenCalledWith(context, 'toolkit'));
      useContextStore.setState({
        selectedContextRef: {
          environment: { kind: 'wsl', distro_name: 'Debian' },
          scope: { scope: 'project', project_id: 'project-2' },
        },
      });
      await useSkillDialogStore.getState().deleteSkill({ fullRemoval: true });

      expect(mockRemoveSkillV2).toHaveBeenCalledWith(context, {
        name: 'toolkit',
        fullRemoval: true,
        agents: undefined,
        agentTargets: undefined,
      });
      expect(mockRemoveSkill).not.toHaveBeenCalled();
    });
  });

  describe('updateSkill', () => {
    it('uses updateSkillV2 for an explicit WSL context', async () => {
      const context = {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        scope: { scope: 'global' },
      } as const;
      mockUpdateSkillV2.mockResolvedValue({
        results: [{
          name: 'toolkit',
          status: 'success',
          warnings: [],
          agentResults: [],
        }],
        summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
      });
      useContextStore.setState({ selectedContextRef: context, hasExplicitContext: true });

      await useSkillsDataStore.getState().updateSkill('toolkit', 'global');

      expect(mockUpdateSkillV2).toHaveBeenCalledWith(context, 'toolkit');
      expect(mockUpdateSkill).not.toHaveBeenCalled();
    });

    it('tracks updating state by scope and name identity', async () => {
      let resolveUpdate: ((value: unknown) => void) | undefined;
      mockUpdateSkill.mockImplementation(
        () =>
          new Promise((resolve) => {
            resolveUpdate = resolve;
          })
      );

      const updatePromise = useSkillsDataStore.getState().updateSkill('toolkit', 'global');

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
        globalSkills: [makeSkill('toolkit', { hasUpdate: true })],
        projectSkills: [],
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
      updateInfoCache.set('global', {
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

      await useSkillsDataStore.getState().updateSkill('toolkit', 'global');

      expect(mockUpdateSkill).toHaveBeenCalledWith({
        scope: 'global',
        name: 'toolkit',
        projectPath: undefined,
      });
      expect(toast.warning).toHaveBeenCalledTimes(2);
      expect(toast.success).not.toHaveBeenCalled();
      expect(toast.error).not.toHaveBeenCalled();

      // Fix 2 回归:partial 不应清缓存,失败 agent 信息应留在 UI
      expect(updateInfoCache.get('global')?.results[0]?.hasUpdate).toBe(true);
      expect(updateInfoCache.get('global')?.results[0]?.status).toBe('update-available');
      // 列表里的 hasUpdate 也应保留
      expect(useSkillsDataStore.getState().globalSkills[0]?.hasUpdate).toBe(true);
    });

    it('refreshes selected skill content after a successful update while keeping identity selection', async () => {
      const updatedSkill = makeSkill('toolkit', {
        hasUpdate: false,
        updatedAt: '2026-04-07T12:00:00.000Z',
      });

      useSkillsDataStore.setState({
        globalSkills: [makeSkill('toolkit', { hasUpdate: true })],
        projectSkills: [],
      });
      useSkillDetailStore.setState({
        selectedSkillRef: { name: 'toolkit', scope: 'global', projectPath: null },
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

      await useSkillsDataStore.getState().updateSkill('toolkit', 'global');

      await vi.waitFor(() => {
        expect(useSkillsDataStore.getState().globalSkills[0]?.hasUpdate).toBe(false);
        expect(useSkillsDataStore.getState().globalSkills[0]?.updatedAt).toBe('2026-04-07T12:00:00.000Z');
        expect(useSkillDetailStore.getState().selectedSkillRef).toEqual({
          name: 'toolkit',
          scope: 'global',
          projectPath: null,
        });
        expect(useSkillDetailStore.getState().skillContent).toBe('# New content');
      });
    });

    it('clears the original project update cache even if context changes before completion', async () => {
      useContextStore.setState({ selectedContext: '/project-a' });
      useSkillsDataStore.setState({
        projectSkills: [
          makeSkill('toolkit', { scope: 'project', canCheckForUpdates: true }),
        ],
      });
      updateInfoCache.set('/project-a', {
        checkedAt: Date.now(),
        results: [{ name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'update-available', gitRef: null }],
      });
      updateInfoCache.set('/project-b', {
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

      const updatePromise = useSkillsDataStore.getState().updateSkill('toolkit', 'project');
      useContextStore.setState({ selectedContext: '/project-b' });

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

      expect(updateInfoCache.get('/project-a')?.results[0]?.hasUpdate).toBe(false);
      expect(updateInfoCache.get('/project-b')?.results[0]?.hasUpdate).toBe(true);
    });

    it('preserves cannot-check cache status when the skill cannot be checked for updates', async () => {
      useSkillsDataStore.setState({
        globalSkills: [
          makeSkill('toolkit', { canCheckForUpdates: false }),
        ],
        projectSkills: [],
      });
      updateInfoCache.set('global', {
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

      await useSkillsDataStore.getState().updateSkill('toolkit', 'global');

      const cached = updateInfoCache.get('global');
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
        globalSkills: [
          {
            ...makeSkill('demo', {
              hasUpdate: false,
              canRunUpdate: true,
              updateReason: 'deleted-upstream',
            }),
            updateStatus: 'deleted-upstream',
          },
        ],
        projectSkills: [],
      });

      await useSkillsDataStore.getState().updateSkill('demo', 'global');

      expect(mockUpdateSkill).not.toHaveBeenCalled();
      expect(toast.info).toHaveBeenCalledWith('skills.updatePlan.deletedUpstreamDescription');
    });

    it('clears missing version metadata after a successful direct reinstall', async () => {
      useSkillsDataStore.setState({
        globalSkills: [
          {
            ...makeSkill('toolkit', {
              canCheckForUpdates: false,
              canRunUpdate: true,
              updateReason: 'missing-remote-hash',
            }),
            updateStatus: 'cannot-check',
          },
        ],
        projectSkills: [],
      });
      updateInfoCache.set('global', {
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

      await useSkillsDataStore.getState().updateSkill('toolkit', 'global');

      const cached = updateInfoCache.get('global');
      expect(cached?.results[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        status: 'up-to-date',
        reason: null,
      }));
      expect(useSkillsDataStore.getState().globalSkills[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        updateStatus: 'up-to-date',
        updateReason: null,
      }));
    });

    it('clears missing source metadata after a successful source repair', () => {
      useSkillsDataStore.setState({
        globalSkills: [
          {
            ...makeSkill('toolkit', {
              canCheckForUpdates: false,
              canRunUpdate: false,
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannot-check',
          },
        ],
        projectSkills: [],
      });
      updateInfoCache.set('global', {
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

      useSkillsDataStore.getState().markSourceRepairSucceeded('toolkit', 'global');

      const cached = updateInfoCache.get('global');
      expect(cached?.results[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        status: 'up-to-date',
        reason: null,
      }));
      expect(useSkillsDataStore.getState().globalSkills[0]).toEqual(expect.objectContaining({
        hasUpdate: false,
        updateStatus: 'up-to-date',
        updateReason: null,
      }));
    });

    it('does not clear the visible project list when source repair completed for another project', () => {
      useContextStore.setState({ selectedContext: '/project-b' });
      useSkillsDataStore.setState({
        globalSkills: [],
        projectSkills: [
          {
            ...makeSkill('toolkit', {
              scope: 'project',
              canCheckForUpdates: false,
              canRunUpdate: false,
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannot-check',
          },
        ],
      });
      updateInfoCache.set('/project-a', {
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

      useSkillsDataStore.getState().markSourceRepairSucceeded('toolkit', 'project', '/project-a');

      const cached = updateInfoCache.get('/project-a');
      expect(cached?.results[0]).toEqual(expect.objectContaining({
        status: 'up-to-date',
        reason: null,
      }));
      expect(useSkillsDataStore.getState().projectSkills[0]).toEqual(expect.objectContaining({
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
        globalSkills: [makeSkill('toolkit', { hasUpdate: true, canRunUpdate: true })],
        projectSkills: [],
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

      await useSkillsDataStore.getState().updateAllInSection('global');

      expect(useSkillsDataStore.getState().lastUpdatePlan?.updatableCount).toBe(1);
      expect(useSkillsDataStore.getState().lastUpdateResults?.[0]).toEqual(
        expect.objectContaining({ name: 'toolkit', status: 'success' })
      );
    });

    it('records repairable legacy skills in the update plan without calling batch update', async () => {
      useSkillsDataStore.setState({
        globalSkills: [
          makeSkill('legacy', {
            hasUpdate: false,
            canRunUpdate: false,
            updateReason: 'missing-skill-path',
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
          }),
        ],
        projectSkills: [],
      });

      await useSkillsDataStore.getState().updateAllInSection('global');

      expect(mockUpdateSkillsBatch).not.toHaveBeenCalled();
      expect(useSkillsDataStore.getState().lastUpdatePlan?.repairableCount).toBe(1);
      expect(useSkillsDataStore.getState().globalSkills[0]?.updateReason).toBe('missing-skill-path');
    });

    it('optimistically clears local hasUpdate flags after successful batch updates', async () => {
      useSkillsDataStore.setState({
        globalSkills: [makeSkill('toolkit', { hasUpdate: true })],
        projectSkills: [],
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

      await useSkillsDataStore.getState().updateAllInSection('global');

      expect(useSkillsDataStore.getState().globalSkills[0]?.hasUpdate).toBe(false);
    });

    it('batches same-source skills separately when their refs differ', async () => {
      useSkillsDataStore.setState({
        globalSkills: [
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
        ],
        projectSkills: [],
      });
      mockListSkills.mockRejectedValue(new Error('sync failed'));
      mockUpdateSkillsBatch.mockImplementation(async ({ names }: { names: string[] }) => ({
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

      await useSkillsDataStore.getState().updateAllInSection('global');

      expect(mockUpdateSkillsBatch).toHaveBeenCalledTimes(2);
      expect(
        mockUpdateSkillsBatch.mock.calls.map(([params]) => params.names)
      ).toEqual([['toolkit-main'], ['toolkit-dev']]);
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
        skillContent: '# Old content',
        loadingContent: false,
      });
      useSkillsDataStore.setState({
        globalSkills: [makeSkill('commit', { canonicalPath: '/fresh/commit' })],
        projectSkills: [],
      });
      mockReadSkillContent.mockResolvedValue('# Fresh content');

      await useSkillDetailStore.getState().reloadContent();

      expect(mockReadSkillContent).toHaveBeenCalledWith('/fresh/commit');
      expect(useSkillDetailStore.getState().skillContent).toBe('# Fresh content');
    });
  });

  describe('manageAgents', () => {
    it('openManageAgents sets target skill and scope', () => {
      const skill = makeSkill('test', { scope: 'project' });
      useContextStore.setState({ selectedContext: '/project-a' });
      useSkillDialogStore.getState().openManageAgents(skill, 'project');
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

      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global' });
      await useSkillDialogStore.getState().saveAgentChanges(['cursor'], [], 'copy');

      expect(mockManageSkillAgents).toHaveBeenCalledWith({
        skillName: 'test',
        scope: 'global',
        projectPath: undefined,
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

      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global' });
      await useSkillDialogStore.getState().saveAgentChanges([], [], 'copy', ['firebender']);

      expect(mockManageSkillAgents).toHaveBeenCalledWith({
        skillName: 'test',
        scope: 'global',
        projectPath: undefined,
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

      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global' });
      await useSkillDialogStore.getState().saveAgentChanges([], [], 'copy', [], ['firebender']);

      expect(mockCleanupDuplicateAgentCopies).toHaveBeenCalledWith({
        skillName: 'test',
        scope: 'global',
        projectPath: undefined,
        agents: ['firebender'],
      });
      expect(mockManageSkillAgents).toHaveBeenCalledWith({
        skillName: 'test',
        scope: 'global',
        projectPath: undefined,
        addAgents: [],
        removeAgents: [],
        privateCopyAgents: [],
        mode: 'copy',
      });
    });

    it('saveAgentChanges keeps the project path from when the dialog was opened', async () => {
      const skill = makeSkill('test', { scope: 'project' });
      useContextStore.setState({ selectedContext: '/project-a' });
      mockManageSkillAgents.mockResolvedValue({ added: [], addedResults: [], removed: [], errors: [] });
      mockListSkills.mockResolvedValue({ skills: [], pathExists: true });

      useSkillDialogStore.getState().openManageAgents(skill, 'project');
      useContextStore.setState({ selectedContext: '/project-b' });

      await useSkillDialogStore.getState().saveAgentChanges(['cursor'], [], 'symlink');

      expect(mockManageSkillAgents).toHaveBeenCalledWith({
        skillName: 'test',
        scope: 'project',
        projectPath: '/project-a',
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

      useSkillDialogStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global' });
      await useSkillDialogStore.getState().saveAgentChanges(['cursor'], [], 'symlink');

      expect(toast.error).toHaveBeenCalled();
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

      expect(mockCleanupDuplicateAgentCopies).toHaveBeenCalledWith({
        skillName: 'test',
        scope: 'global',
        projectPath: undefined,
        agents: ['firebender'],
      });
      expect(mockGetAgentDetails).toHaveBeenCalledWith({ scope: 'global', name: 'test', projectPath: undefined });
      expect(syncSkills).toHaveBeenCalledTimes(1);
      expect(useSkillDialogStore.getState().manageAgentDetails).toEqual(refreshedDetails);
      expect(toast.success).toHaveBeenCalled();
    });
  });

  describe('copyToProject', () => {
    it('openCopyToProject sets target skill', () => {
      const skill = makeSkill('test', { scope: 'project' });
      useSkillDialogStore.getState().openCopyToProject(skill);
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

      useSkillDialogStore.setState({ copySkill: skill });
      await useSkillDialogStore.getState().executeCopy(['/project-b']);

      expect(mockCopySkillToProjects).toHaveBeenCalledWith({
        skillName: 'test',
        sourceProjectPath: 'global',
        targetProjectPaths: ['/project-b'],
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

      useSkillDialogStore.setState({ copySkill: skill });
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

      useSkillDialogStore.setState({ copySkill: skill });
      await useSkillDialogStore.getState().executeCopy(['/a', '/b']);

      expect(toast.error).toHaveBeenCalled();
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

      useSkillDialogStore.setState({ copySkill: skill });
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

      useSkillDialogStore.setState({ copySkill: skill });
      await useSkillDialogStore.getState().executeCopy(['/a', '/b']);

      expect(toast.error).toHaveBeenCalled();
      expect(toast.warning).not.toHaveBeenCalledWith('skills.copyToProject.metadataIncomplete');
    });
  });
});
