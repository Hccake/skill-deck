// src/stores/__tests__/skills.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { InstalledSkill, SkillAgentDetails } from '@/bindings';
import { toast } from 'sonner';
import { useSkillsDataStore } from '../skills-data';
import { useSkillDetailStore } from '../skill-detail';
import { useSkillDialogStore } from '../skill-dialog';
import { useContextStore } from '../context';
import { buildUpdatePlan, mergeUpdateInfo, updateInfoCache } from '../skills-utils';

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
  copySkillToProjects: (...args: unknown[]) => mockCopySkillToProjects(...args),
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

describe('useSkillsStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useContextStore.setState({ selectedContext: 'global' });
    mockListSkills.mockResolvedValue({ skills: [], pathExists: true });
    mockCheckUpdates.mockResolvedValue([]);
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
      isSyncing: false,
      checkingUpdateScopes: new Set(),
      updatingSkills: new Map(),
      updateAllCancelled: false,
      lastUpdatePlan: null,
      lastUpdateResults: null,
      lastFailedUpdateNames: [],
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
      copySkill: null,
    });
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
      mockListAgents.mockResolvedValue([]);
      mockListSkills
        .mockResolvedValueOnce({ skills: [makeSkill('global-skill')], pathExists: true })
        .mockResolvedValueOnce({ skills: [makeSkill('project-skill')], pathExists: true });

      await useSkillsDataStore.getState().fetchSkills();

      const state = useSkillsDataStore.getState();
      expect(state.globalSkills).toHaveLength(1);
      expect(state.projectSkills).toHaveLength(1);
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
          source: 'owner/repo',
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

    it('openDelete sets deleteTarget and fetches agent details', async () => {
      const skill = makeSkill('test-skill');
      const details: SkillAgentDetails = { skillName: 'test-skill', scope: 'global', canonicalPath: '/tmp', automaticAgents: [], independentAgents: [] };
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
        agentDetails: { skillName: 'x', scope: 'global', canonicalPath: '/tmp', automaticAgents: [], independentAgents: [] } satisfies SkillAgentDetails,
        loadingAgentDetails: true,
      });

      useSkillDialogStore.getState().closeDelete();

      expect(useSkillDialogStore.getState().deleteTarget).toBeNull();
      expect(useSkillDialogStore.getState().agentDetails).toBeNull();
      expect(useSkillDialogStore.getState().loadingAgentDetails).toBe(false);
    });
  });

  describe('updateSkill', () => {
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
        mode: 'copy',
      });
      expect(useSkillDialogStore.getState().manageAgentsSkill).toBeNull();
      expect(toast.success).toHaveBeenCalled();
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
      const skill = makeSkill('test', { scope: 'project', agents: ['claude-code'] });
      mockCopySkillToProjects.mockResolvedValue({
        results: [{ projectPath: '/project-b', success: true, error: null }],
      });

      useSkillDialogStore.setState({ copySkill: skill });
      await useSkillDialogStore.getState().executeCopy(['/project-b']);

      expect(mockCopySkillToProjects).toHaveBeenCalledWith({
        skillName: 'test',
        sourceProjectPath: 'global',
        targetProjectPaths: ['/project-b'],
        agents: ['claude-code'],
      });
      expect(useSkillDialogStore.getState().copySkill).toBeNull();
      expect(toast.success).toHaveBeenCalled();
    });

    it('executeCopy shows error toast on partial failure', async () => {
      const skill = makeSkill('test', { scope: 'project' });
      mockCopySkillToProjects.mockResolvedValue({
        results: [
          { projectPath: '/a', success: true, error: null },
          { projectPath: '/b', success: false, error: 'disk full' },
        ],
      });

      useSkillDialogStore.setState({ copySkill: skill });
      await useSkillDialogStore.getState().executeCopy(['/a', '/b']);

      expect(toast.error).toHaveBeenCalled();
    });
  });
});
