// src/stores/__tests__/skills.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { InstalledSkill, SkillAgentDetails } from '@/bindings';
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

vi.mock('@/hooks/useTauriApi', () => ({
  listSkills: (...args: unknown[]) => mockListSkills(...args),
  listAgents: (...args: unknown[]) => mockListAgents(...args),
  removeSkill: (...args: unknown[]) => mockRemoveSkill(...args),
  getSkillAgentDetails: (...args: unknown[]) => mockGetAgentDetails(...args),
  checkUpdates: (...args: unknown[]) => mockCheckUpdates(...args),
  updateSkill: (...args: unknown[]) => mockUpdateSkill(...args),
  openInstallWizard: (...args: unknown[]) => mockOpenInstallWizard(...args),
  checkSkillAudit: (...args: unknown[]) => mockCheckSkillAudit(...args),
}));

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
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
    useSkillsStore.setState({
      globalSkills: [],
      projectSkills: [],
      projectPathExists: true,
      allAgents: [],
      loading: true,
      error: null,
      auditCache: {},
      isSyncing: false,
      updatingSkill: null,
      detailSkill: null,
      deleteTarget: null,
      agentDetails: null,
      loadingAgentDetails: false,
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
    it('openDetail sets detailSkill, closeDetail clears it', () => {
      const skill = makeSkill('test-skill');
      useSkillsStore.getState().openDetail(skill);
      expect(useSkillsStore.getState().detailSkill).toEqual(skill);

      useSkillsStore.getState().closeDetail();
      expect(useSkillsStore.getState().detailSkill).toBeNull();
    });

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
});
