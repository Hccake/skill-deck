// src/stores/__tests__/skills.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { InstalledSkill, SkillAgentDetails } from '@/bindings';
import { toast } from 'sonner';
import { useSkillsStore } from '../skills';
import { useContextStore } from '../context';

const mockListSkills = vi.fn();
const mockListAgents = vi.fn();
const mockRemoveSkill = vi.fn();
const mockGetAgentDetails = vi.fn();
const mockCheckUpdates = vi.fn();
const mockUpdateSkill = vi.fn();
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
    useSkillsStore.setState({
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
      deleteTarget: null,
      agentDetails: null,
      loadingAgentDetails: false,
      selectedSkill: null,
      skillContent: null,
      loadingContent: false,
      manageAgentsSkill: null,
      manageAgentsScope: 'global',
      copySkill: null,
    });
  });

  describe('fetchSkills — global scope', () => {
    it('loads global skills when context is global', async () => {
      const skills = [makeSkill('toolkit'), makeSkill('analyzer')];
      mockListAgents.mockResolvedValue([]);
      mockListSkills.mockResolvedValue({ skills, pathExists: true });

      await useSkillsStore.getState().fetchSkills();

      const state = useSkillsStore.getState();
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

      await useSkillsStore.getState().fetchSkills();

      const state = useSkillsStore.getState();
      expect(state.globalSkills).toHaveLength(1);
      expect(state.projectSkills).toHaveLength(1);
    });
  });

  describe('fetchSkills — error handling', () => {
    it('sets error state on failure', async () => {
      mockListAgents.mockRejectedValue(new Error('network down'));

      await useSkillsStore.getState().fetchSkills();

      expect(useSkillsStore.getState().error).toBe('network down');
      expect(useSkillsStore.getState().loading).toBe(false);
    });
  });

  describe('dialog state', () => {
    it('openDelete sets deleteTarget and fetches agent details', async () => {
      const skill = makeSkill('test-skill');
      const details: SkillAgentDetails = { skillName: 'test-skill', scope: 'global', canonicalPath: '/tmp', universalAgents: [], independentAgents: [] };
      mockGetAgentDetails.mockResolvedValue(details);

      useSkillsStore.getState().openDelete(skill, 'global');

      expect(useSkillsStore.getState().deleteTarget).toBeTruthy();
      expect(useSkillsStore.getState().deleteTarget!.skill.name).toBe('test-skill');
      expect(useSkillsStore.getState().loadingAgentDetails).toBe(true);

      await vi.waitFor(() => {
        expect(useSkillsStore.getState().loadingAgentDetails).toBe(false);
      });
      expect(useSkillsStore.getState().agentDetails).toEqual(details);
    });

    it('closeDelete clears all delete state', () => {
      useSkillsStore.setState({
        deleteTarget: { skill: makeSkill('x'), scope: 'global' },
        agentDetails: { skillName: 'x', scope: 'global', canonicalPath: '/tmp', universalAgents: [], independentAgents: [] } satisfies SkillAgentDetails,
        loadingAgentDetails: true,
      });

      useSkillsStore.getState().closeDelete();

      expect(useSkillsStore.getState().deleteTarget).toBeNull();
      expect(useSkillsStore.getState().agentDetails).toBeNull();
      expect(useSkillsStore.getState().loadingAgentDetails).toBe(false);
    });
  });

  describe('updateSkill', () => {
    it('shows partial + warning feedback using update response details', async () => {
      useSkillsStore.setState({
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

      await useSkillsStore.getState().updateSkill('toolkit', 'global');

      expect(mockUpdateSkill).toHaveBeenCalledWith({
        scope: 'global',
        name: 'toolkit',
        projectPath: undefined,
      });
      expect(toast.warning).toHaveBeenCalledTimes(2);
      expect(toast.success).not.toHaveBeenCalled();
      expect(toast.error).not.toHaveBeenCalled();
    });
  });

  describe('selectSkill / deselectSkill', () => {
    it('selectSkill sets selectedSkill and loads content', async () => {
      const skill = makeSkill('commit');
      mockReadSkillContent.mockResolvedValue('# Commit\n\nBody content');

      await useSkillsStore.getState().selectSkill(skill);

      const state = useSkillsStore.getState();
      expect(state.selectedSkill).toEqual(skill);
      expect(state.skillContent).toBe('# Commit\n\nBody content');
      expect(state.loadingContent).toBe(false);
    });

    it('selectSkill ignores stale response on fast switch', async () => {
      const skill1 = makeSkill('skill-1');
      const skill2 = makeSkill('skill-2');

      mockReadSkillContent
        .mockImplementationOnce(() => new Promise((resolve) => setTimeout(() => resolve('content-1'), 100)))
        .mockImplementationOnce(() => Promise.resolve('content-2'));

      const p1 = useSkillsStore.getState().selectSkill(skill1);
      const p2 = useSkillsStore.getState().selectSkill(skill2);
      await Promise.all([p1, p2]);

      const state = useSkillsStore.getState();
      expect(state.selectedSkill?.name).toBe('skill-2');
      expect(state.skillContent).toBe('content-2');
    });

    it('selectSkill same skill is no-op', async () => {
      const skill = makeSkill('commit');
      mockReadSkillContent.mockResolvedValue('content');

      await useSkillsStore.getState().selectSkill(skill);
      mockReadSkillContent.mockClear();

      await useSkillsStore.getState().selectSkill(skill);
      expect(mockReadSkillContent).not.toHaveBeenCalled();
    });

    it('deselectSkill clears selection', async () => {
      const skill = makeSkill('commit');
      mockReadSkillContent.mockResolvedValue('content');
      await useSkillsStore.getState().selectSkill(skill);

      useSkillsStore.getState().deselectSkill();

      const state = useSkillsStore.getState();
      expect(state.selectedSkill).toBeNull();
      expect(state.skillContent).toBeNull();
      expect(state.loadingContent).toBe(false);
    });

    it('selectSkill handles load error gracefully', async () => {
      const skill = makeSkill('broken');
      mockReadSkillContent.mockRejectedValue(new Error('read failed'));

      await useSkillsStore.getState().selectSkill(skill);

      const state = useSkillsStore.getState();
      expect(state.selectedSkill).toEqual(skill);
      expect(state.skillContent).toBeNull();
      expect(state.loadingContent).toBe(false);
    });
  });

  describe('manageAgents', () => {
    it('openManageAgents sets target skill and scope', () => {
      const skill = makeSkill('test', { scope: 'project' });
      useSkillsStore.getState().openManageAgents(skill, 'project');
      expect(useSkillsStore.getState().manageAgentsSkill).toBe(skill);
      expect(useSkillsStore.getState().manageAgentsScope).toBe('project');
    });

    it('closeManageAgents clears target', () => {
      const skill = makeSkill('test');
      useSkillsStore.setState({ manageAgentsSkill: skill });
      useSkillsStore.getState().closeManageAgents();
      expect(useSkillsStore.getState().manageAgentsSkill).toBeNull();
    });

    it('saveAgentChanges calls API and syncs skills', async () => {
      const skill = makeSkill('test');
      mockManageSkillAgents.mockResolvedValue({ added: ['cursor'], removed: [], errors: [] });
      mockListSkills.mockResolvedValue({ skills: [makeSkill('test', { agents: ['claude-code', 'cursor'] })], pathExists: true });
      mockListAgents.mockResolvedValue([]);

      useSkillsStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global' });
      await useSkillsStore.getState().saveAgentChanges(['cursor'], []);

      expect(mockManageSkillAgents).toHaveBeenCalledWith({
        skillName: 'test',
        scope: 'global',
        projectPath: undefined,
        addAgents: ['cursor'],
        removeAgents: [],
      });
      expect(useSkillsStore.getState().manageAgentsSkill).toBeNull();
      expect(toast.success).toHaveBeenCalled();
    });

    it('saveAgentChanges shows error toast on API errors', async () => {
      const skill = makeSkill('test');
      mockManageSkillAgents.mockResolvedValue({ added: [], removed: [], errors: ['cursor: failed'] });
      mockListSkills.mockResolvedValue({ skills: [], pathExists: true });

      useSkillsStore.setState({ manageAgentsSkill: skill, manageAgentsScope: 'global' });
      await useSkillsStore.getState().saveAgentChanges(['cursor'], []);

      expect(toast.error).toHaveBeenCalled();
    });
  });

  describe('copyToProject', () => {
    it('openCopyToProject sets target skill', () => {
      const skill = makeSkill('test', { scope: 'project' });
      useSkillsStore.getState().openCopyToProject(skill);
      expect(useSkillsStore.getState().copySkill).toBe(skill);
    });

    it('closeCopyToProject clears target', () => {
      useSkillsStore.setState({ copySkill: makeSkill('test') });
      useSkillsStore.getState().closeCopyToProject();
      expect(useSkillsStore.getState().copySkill).toBeNull();
    });

    it('executeCopy calls API and shows success toast', async () => {
      const skill = makeSkill('test', { scope: 'project', agents: ['claude-code'] });
      mockCopySkillToProjects.mockResolvedValue({
        results: [{ projectPath: '/project-b', success: true, error: null }],
      });

      useSkillsStore.setState({ copySkill: skill });
      await useSkillsStore.getState().executeCopy(['/project-b']);

      expect(mockCopySkillToProjects).toHaveBeenCalledWith({
        skillName: 'test',
        sourceProjectPath: 'global',
        targetProjectPaths: ['/project-b'],
        agents: ['claude-code'],
      });
      expect(useSkillsStore.getState().copySkill).toBeNull();
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

      useSkillsStore.setState({ copySkill: skill });
      await useSkillsStore.getState().executeCopy(['/a', '/b']);

      expect(toast.error).toHaveBeenCalled();
    });
  });
});
