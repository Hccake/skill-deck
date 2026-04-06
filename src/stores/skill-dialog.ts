// src/stores/skill-dialog.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import { useContextStore } from './context';
import { t, type DeleteTarget, type AddDialogPrefill } from './skills-utils';
import {
  removeSkill as apiRemoveSkill,
  getSkillAgentDetails as apiGetAgentDetails,
  openInstallWizard,
  manageSkillAgents as apiManageSkillAgents,
  copySkillToProjects as apiCopySkillToProjects,
} from '@/hooks/useTauriApi';
import type { AgentType, InstalledSkill, SkillScope, SkillAgentDetails } from '@/bindings';

interface SkillDialogState {
  // Delete dialog
  deleteTarget: DeleteTarget | null;
  agentDetails: SkillAgentDetails | null;
  loadingAgentDetails: boolean;

  // Manage agents dialog
  manageAgentsSkill: InstalledSkill | null;
  manageAgentsScope: SkillScope;

  // Copy to project dialog
  copySkill: InstalledSkill | null;

  // Actions
  openDelete: (skill: InstalledSkill, scope: SkillScope, projectPath?: string) => void;
  closeDelete: () => void;
  deleteSkill: (params: { fullRemoval: boolean; agents?: AgentType[] }) => Promise<void>;
  openAdd: (scope: SkillScope) => void;
  openAddWithPrefill: (prefill: AddDialogPrefill) => void;
  openManageAgents: (skill: InstalledSkill, scope: SkillScope) => void;
  closeManageAgents: () => void;
  saveAgentChanges: (addAgents: string[], removeAgents: string[]) => Promise<void>;
  openCopyToProject: (skill: InstalledSkill) => void;
  closeCopyToProject: () => void;
  executeCopy: (targetPaths: string[]) => Promise<void>;
}

export const useSkillDialogStore = create<SkillDialogState>()((set, get) => ({
  deleteTarget: null,
  agentDetails: null,
  loadingAgentDetails: false,
  manageAgentsSkill: null,
  manageAgentsScope: 'global' as SkillScope,
  copySkill: null,

  openDelete: (skill, scope, projectPath) => {
    set({
      deleteTarget: { skill, scope, projectPath },
      agentDetails: null,
      loadingAgentDetails: true,
    });
    apiGetAgentDetails({ scope, name: skill.name, projectPath })
      .then((details) => set({ agentDetails: details }))
      .catch((e) => console.warn('Failed to fetch agent details:', e))
      .finally(() => set({ loadingAgentDetails: false }));
  },

  closeDelete: () => set({ deleteTarget: null, agentDetails: null, loadingAgentDetails: false }),

  deleteSkill: async ({ fullRemoval, agents }) => {
    const { deleteTarget } = get();
    if (!deleteTarget) return;

    try {
      await apiRemoveSkill({
        scope: deleteTarget.scope,
        name: deleteTarget.skill.name,
        projectPath: deleteTarget.projectPath,
        fullRemoval,
        agents,
      });
      const msg = fullRemoval
        ? t('skills.deleteSuccess', { name: deleteTarget.skill.name })
        : t('skills.partialDeleteSuccess', { name: deleteTarget.skill.name, count: agents?.length ?? 0 });
      toast.success(msg);

      // Auto-deselect if the deleted skill was selected
      const { useSkillDetailStore } = await import('./skill-detail');
      const detailState = useSkillDetailStore.getState();
      if (detailState.selectedSkill && detailState.selectedSkill.name === deleteTarget.skill.name) {
        detailState.deselectSkill();
      }

      set({ deleteTarget: null, agentDetails: null });

      // Refresh skills list
      const { useSkillsDataStore } = await import('./skills-data');
      await useSkillsDataStore.getState().fetchSkills();
    } catch (e) {
      toast.error(t('skills.deleteError', {
        name: deleteTarget.skill.name,
        error: e instanceof Error ? e.message : String(e),
      }));
      set({ deleteTarget: null, agentDetails: null });
    }
  },

  openAdd: (scope) => {
    const { selectedContext } = useContextStore.getState();
    openInstallWizard({
      entryPoint: 'skills-panel',
      scope,
      projectPath: scope === 'project' ? selectedContext : undefined,
    }).catch((e) => {
      console.error('[openAdd] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },

  openAddWithPrefill: (prefill) => {
    openInstallWizard({
      entryPoint: 'discovery',
      scope: 'global',
      prefillSource: prefill.source,
      prefillSkillName: prefill.skillName,
    }).catch((e) => {
      console.error('[openAddWithPrefill] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },

  openManageAgents: (skill, scope) => {
    set({ manageAgentsSkill: skill, manageAgentsScope: scope });
  },

  closeManageAgents: () => set({ manageAgentsSkill: null }),

  saveAgentChanges: async (addAgents, removeAgents) => {
    const { manageAgentsSkill, manageAgentsScope } = get();
    if (!manageAgentsSkill) return;

    const { selectedContext } = useContextStore.getState();
    const scope = manageAgentsScope === 'project' ? 'project' : 'global';
    const projectPath = scope === 'project' ? selectedContext : undefined;

    try {
      const result = await apiManageSkillAgents({
        skillName: manageAgentsSkill.name,
        scope,
        projectPath,
        addAgents: addAgents as AgentType[],
        removeAgents: removeAgents as AgentType[],
      });

      if (result.errors.length > 0) {
        toast.error(result.errors.join('\n'));
      } else {
        toast.success(t('skills.manageAgents.success'));
      }

      set({ manageAgentsSkill: null });

      // Refresh skills list and detail panel
      const { useSkillsDataStore } = await import('./skills-data');
      await useSkillsDataStore.getState().syncSkills();

      const { useSkillDetailStore } = await import('./skill-detail');
      const { selectedSkill } = useSkillDetailStore.getState();
      if (selectedSkill) {
        const { globalSkills, projectSkills } = useSkillsDataStore.getState();
        const allSkills = [...globalSkills, ...projectSkills];
        const updated = allSkills.find((s) => s.name === selectedSkill.name && s.scope === selectedSkill.scope);
        if (updated) {
          useSkillDetailStore.setState({ selectedSkill: updated });
        }
      }
    } catch (e) {
      console.error('[saveAgentChanges] Failed:', e);
      toast.error(String(e));
    }
  },

  openCopyToProject: (skill) => {
    set({ copySkill: skill });
  },

  closeCopyToProject: () => set({ copySkill: null }),

  executeCopy: async (targetPaths) => {
    const { copySkill } = get();
    if (!copySkill) return;

    const { selectedContext } = useContextStore.getState();

    try {
      const result = await apiCopySkillToProjects({
        skillName: copySkill.name,
        sourceProjectPath: selectedContext,
        targetProjectPaths: targetPaths,
        agents: copySkill.agents,
      });

      const successCount = result.results.filter((r) => r.success).length;
      const failCount = result.results.filter((r) => !r.success).length;

      if (failCount > 0) {
        const errors = result.results
          .filter((r) => !r.success)
          .map((r) => `${r.projectPath}: ${r.error}`)
          .join('\n');
        toast.error(t('skills.copyToProject.partialError', { success: successCount, fail: failCount }) + '\n' + errors);
      } else {
        toast.success(t('skills.copyToProject.success', { count: successCount }));
      }

      set({ copySkill: null });
    } catch (e) {
      console.error('[executeCopy] Failed:', e);
      toast.error(String(e));
    }
  },
}));
