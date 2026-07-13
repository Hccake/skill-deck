// src/stores/skill-dialog.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import { useContextStore } from './context';
import {
  createSkillRepairDraft,
  getSkillOperationAgents,
  t,
  type AddDialogPrefill,
  type DeleteTarget,
  type RepairSourceDraft,
} from './skills-utils';
import { getSkillIdentity, isSameSkillIdentity } from '@/lib/skills/identity';
import {
  removeSkill as apiRemoveSkill,
  removeSkillV2 as apiRemoveSkillV2,
  getSkillAgentDetails as apiGetAgentDetails,
  getSkillAgentDetailsV2 as apiGetAgentDetailsV2,
  openInstallWizard,
  manageSkillAgents as apiManageSkillAgents,
  manageSkillAgentsV2 as apiManageSkillAgentsV2,
  cleanupDuplicateAgentCopies as apiCleanupDuplicateAgentCopies,
  cleanupDuplicateAgentCopiesV2 as apiCleanupDuplicateAgentCopiesV2,
  copySkillToProjects as apiCopySkillToProjects,
  copySkillToProjectsV2 as apiCopySkillToProjectsV2,
} from '@/hooks/useTauriApi';
import { useEnvironmentStore, environmentKey } from './environment';
import type {
  AgentType,
  InstalledSkill,
  SkillScope,
  SkillAgentDetails,
  InstallMode,
  InstallTargetSpec,
  ContextRef,
} from '@/bindings';

interface SkillDialogState {
  // Delete dialog
  deleteTarget: DeleteTarget | null;
  agentDetails: SkillAgentDetails | null;
  loadingAgentDetails: boolean;

  // Manage agents dialog
  manageAgentsSkill: InstalledSkill | null;
  manageAgentsScope: SkillScope;
  manageAgentsProjectPath?: string;
  manageAgentsContext?: ContextRef;
  manageAgentDetails: SkillAgentDetails | null;
  loadingManageAgentDetails: boolean;

  // Copy to project dialog
  copySkill: InstalledSkill | null;
  copyContext?: ContextRef;

  // Repair source dialog
  repairSourceTarget: RepairSourceDraft | null;

  // Actions
  openDelete: (skill: InstalledSkill, scope: SkillScope, projectPath?: string) => void;
  closeDelete: () => void;
  deleteSkill: (params: {
    fullRemoval: boolean;
    agents?: AgentType[];
    agentTargets?: InstallTargetSpec[];
  }) => Promise<void>;
  openAdd: (scope: SkillScope) => void;
  openAddWithPrefill: (prefill: AddDialogPrefill) => void;
  openRepairSource: (
    skill: InstalledSkill,
    scope: SkillScope,
    projectPath?: string,
    context?: ContextRef,
  ) => void;
  closeRepairSource: () => void;
  openManageAgents: (skill: InstalledSkill, scope: SkillScope) => void;
  closeManageAgents: () => void;
  saveAgentChanges: (
    addAgents: string[],
    removeAgents: string[],
    mode: InstallMode,
    privateCopyAgents?: string[],
    removePrivateCopyAgents?: string[],
  ) => Promise<void>;
  cleanupDuplicateCopies: (agents: string[]) => Promise<void>;
  openCopyToProject: (skill: InstalledSkill) => void;
  closeCopyToProject: () => void;
  executeCopy: (targetPaths: string[]) => Promise<void>;
}

function getExplicitContextForScope(scope: SkillScope): ContextRef | null {
  const { hasExplicitContext, selectedContextRef } = useContextStore.getState();
  if (!hasExplicitContext) return null;
  if (scope === 'global') {
    return {
      environment: selectedContextRef.environment,
      scope: { scope: 'global' },
    };
  }
  return selectedContextRef.scope.scope === 'project' ? selectedContextRef : null;
}

export const useSkillDialogStore = create<SkillDialogState>()((set, get) => ({
  deleteTarget: null,
  agentDetails: null,
  loadingAgentDetails: false,
  manageAgentsSkill: null,
  manageAgentsScope: 'global' as SkillScope,
  manageAgentsProjectPath: undefined,
  manageAgentsContext: undefined,
  manageAgentDetails: null,
  loadingManageAgentDetails: false,
  copySkill: null,
  copyContext: undefined,
  repairSourceTarget: null,

  openDelete: (skill, scope, projectPath) => {
    const context = getExplicitContextForScope(scope);
    set({
      deleteTarget: { skill, scope, projectPath, context: context ?? undefined },
      agentDetails: null,
      loadingAgentDetails: true,
    });
    (context
      ? apiGetAgentDetailsV2(context, skill.name)
      : apiGetAgentDetails({ scope, name: skill.name, projectPath }))
      .then((details) => set({ agentDetails: details }))
      .catch((e) => console.warn('Failed to fetch agent details:', e))
      .finally(() => set({ loadingAgentDetails: false }));
  },

  closeDelete: () => set({ deleteTarget: null, agentDetails: null, loadingAgentDetails: false }),

  deleteSkill: async ({ fullRemoval, agents, agentTargets }) => {
    const { deleteTarget } = get();
    if (!deleteTarget) return;

    try {
      const context = deleteTarget.context;
      if (context) {
        await apiRemoveSkillV2(context, {
          name: deleteTarget.skill.name,
          fullRemoval,
          agents,
          agentTargets,
        });
      } else {
        await apiRemoveSkill({
          scope: deleteTarget.scope,
          name: deleteTarget.skill.name,
          projectPath: deleteTarget.projectPath,
          fullRemoval,
          agents,
          agentTargets,
        });
      }
      const removedTargetCount = (agents?.length ?? 0) + (agentTargets?.length ?? 0);
      const msg = fullRemoval
        ? t('skills.deleteSuccess', { name: deleteTarget.skill.name })
        : t('skills.partialDeleteSuccess', { name: deleteTarget.skill.name, count: removedTargetCount });
      toast.success(msg);

      // Auto-deselect if the deleted skill was selected
      const { useSkillDetailStore } = await import('./skill-detail');
      const detailState = useSkillDetailStore.getState();
      const deletedSkillIdentity = getSkillIdentity(
        deleteTarget.skill,
        deleteTarget.scope === 'project' ? deleteTarget.projectPath : undefined
      );
      if (isSameSkillIdentity(detailState.selectedSkillRef, deletedSkillIdentity)) {
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
    const context = getExplicitContextForScope(scope);
    openInstallWizard({
      entryPoint: 'skills-panel',
      scope,
      projectPath: scope === 'project' ? selectedContext : undefined,
      context: context ?? undefined,
    }).catch((e) => {
      console.error('[openAdd] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },

  openAddWithPrefill: (prefill) => {
    const scope = prefill.scope ?? 'global';
    const selectedContext = useContextStore.getState().selectedContext;
    const context = getExplicitContextForScope(scope);
    const projectPath =
      scope === 'project'
        ? prefill.projectPath ?? (selectedContext !== 'global' ? selectedContext : undefined)
        : undefined;
    openInstallWizard({
      entryPoint: 'discovery',
      scope,
      projectPath,
      prefillSource: prefill.source,
      prefillSkillName: prefill.skillName,
      context: context ?? undefined,
    }).catch((e) => {
      console.error('[openAddWithPrefill] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },

  openRepairSource: (skill, scope, projectPath, context) => {
    const repairSourceTarget = createSkillRepairDraft(
      skill,
      scope,
      projectPath,
      context ?? getExplicitContextForScope(scope) ?? undefined,
    );
    if (!repairSourceTarget) return;
    set({ repairSourceTarget });
  },

  closeRepairSource: () => set({ repairSourceTarget: null }),

  openManageAgents: (skill, scope) => {
    const manageAgentsProjectPath =
      scope === 'project' ? useContextStore.getState().selectedContext : undefined;
    const context = getExplicitContextForScope(scope);
    set({
      manageAgentsSkill: skill,
      manageAgentsScope: scope,
      manageAgentsProjectPath,
      manageAgentsContext: context ?? undefined,
      manageAgentDetails: null,
      loadingManageAgentDetails: true,
    });
    (context
      ? apiGetAgentDetailsV2(context, skill.name)
      : apiGetAgentDetails({ scope, name: skill.name, projectPath: manageAgentsProjectPath }))
      .then((details) => set({ manageAgentDetails: details }))
      .catch((e) => console.warn('Failed to fetch manage agent details:', e))
      .finally(() => set({ loadingManageAgentDetails: false }));
  },

  closeManageAgents: () => set({
    manageAgentsSkill: null,
    manageAgentsProjectPath: undefined,
    manageAgentsContext: undefined,
    manageAgentDetails: null,
    loadingManageAgentDetails: false,
  }),

  saveAgentChanges: async (addAgents, removeAgents, mode, privateCopyAgents = [], removePrivateCopyAgents = []) => {
    const { manageAgentsSkill, manageAgentsScope, manageAgentsProjectPath, manageAgentsContext } = get();
    if (!manageAgentsSkill) return;

    const scope = manageAgentsScope === 'project' ? 'project' : 'global';
    const projectPath = scope === 'project' ? manageAgentsProjectPath : undefined;
    const context = manageAgentsContext;

    try {
      if (removePrivateCopyAgents.length > 0) {
        const cleanupResults = context
          ? await apiCleanupDuplicateAgentCopiesV2(context, {
            skillName: manageAgentsSkill.name,
            agents: removePrivateCopyAgents as AgentType[],
          })
          : await apiCleanupDuplicateAgentCopies({
            skillName: manageAgentsSkill.name,
            scope,
            projectPath,
            agents: removePrivateCopyAgents as AgentType[],
          });
        const cleanupFailures = cleanupResults.filter((result) => !result.success && !result.skipped);
        if (cleanupFailures.length > 0) {
          toast.error(cleanupFailures.map((result) => `${result.agent}: ${result.error}`).join('\n'));
          return;
        }
      }

      const result = context
        ? await apiManageSkillAgentsV2(context, {
          skillName: manageAgentsSkill.name,
          addAgents: addAgents as AgentType[],
          removeAgents: removeAgents as AgentType[],
          privateCopyAgents: privateCopyAgents as AgentType[],
          mode,
        })
        : await apiManageSkillAgents({
          skillName: manageAgentsSkill.name,
          scope,
          projectPath,
          addAgents: addAgents as AgentType[],
          removeAgents: removeAgents as AgentType[],
          privateCopyAgents: privateCopyAgents as AgentType[],
          mode,
        });

      const addResultErrors = result.addedResults
        .filter((item) => !item.success && item.error)
        .map((item) => `${item.agent}: ${item.error}`);
      const errors = result.errors.length > 0 ? result.errors : addResultErrors;

      if (errors.length > 0) {
        toast.error(errors.join('\n'));
      } else {
        toast.success(t('skills.manageAgents.success'));
      }

      set({ manageAgentsSkill: null, manageAgentsProjectPath: undefined, manageAgentsContext: undefined });

      // Refresh skills list and detail panel
      const { useSkillsDataStore } = await import('./skills-data');
      await useSkillsDataStore.getState().syncSkills();
    } catch (e) {
      console.error('[saveAgentChanges] Failed:', e);
      toast.error(String(e));
    }
  },

  cleanupDuplicateCopies: async (agents) => {
    const { manageAgentsSkill, manageAgentsScope, manageAgentsProjectPath, manageAgentsContext } = get();
    if (!manageAgentsSkill || agents.length === 0) return;

    const scope = manageAgentsScope === 'project' ? 'project' : 'global';
    const projectPath = scope === 'project' ? manageAgentsProjectPath : undefined;
    const context = manageAgentsContext;

    try {
      const results = context
        ? await apiCleanupDuplicateAgentCopiesV2(context, {
          skillName: manageAgentsSkill.name,
          agents: agents as AgentType[],
        })
        : await apiCleanupDuplicateAgentCopies({
          skillName: manageAgentsSkill.name,
          scope,
          projectPath,
          agents: agents as AgentType[],
        });
      const failures = results.filter((result) => !result.success && !result.skipped);
      if (failures.length > 0) {
        toast.error(failures.map((result) => `${result.agent}: ${result.error}`).join('\n'));
      } else {
        toast.success(t('skills.manageAgents.cleanupSuccess'));
      }

      const [details] = await Promise.all([
        context
          ? apiGetAgentDetailsV2(context, manageAgentsSkill.name)
          : apiGetAgentDetails({ scope, name: manageAgentsSkill.name, projectPath }),
        import('./skills-data').then(({ useSkillsDataStore }) =>
          useSkillsDataStore.getState().syncSkills()
        ),
      ]);
      set({ manageAgentDetails: details });
    } catch (e) {
      console.error('[cleanupDuplicateCopies] Failed:', e);
      toast.error(String(e));
    }
  },

  openCopyToProject: (skill) => {
    set({
      copySkill: skill,
      copyContext: getExplicitContextForScope('project') ?? undefined,
    });
  },

  closeCopyToProject: () => set({ copySkill: null, copyContext: undefined }),

  executeCopy: async (targetPaths) => {
    const { copySkill, copyContext } = get();
    if (!copySkill) return;

    const { selectedContext } = useContextStore.getState();
    const context = copyContext;

    try {
      const agents = copySkill.privateAdaptedAgents ?? getSkillOperationAgents(copySkill);
      const privateCopyAgents = copySkill.privateCopyAgents ?? [];
      const result = context
        ? await (async () => {
          const { projectsByEnvironment } = useEnvironmentStore.getState();
          const bindings = projectsByEnvironment[environmentKey(context.environment)] ?? [];
          const byPath = new Map(bindings.map((project) => [project.nativePath, project]));
          const targets = targetPaths.map((path) => byPath.get(path));
          if (context.scope.scope !== 'project' || targets.some((project) => !project)) {
            throw new Error('Selected projects are not available in the current environment');
          }
          return apiCopySkillToProjectsV2({
            skillName: copySkill.name,
            source: context,
            targets: targets.map((project) => ({
              environment: context.environment,
              scope: { scope: 'project', project_id: project!.id },
            })),
            agents,
            privateCopyAgents,
          });
        })()
        : await apiCopySkillToProjects({
          skillName: copySkill.name,
          sourceProjectPath: selectedContext,
          targetProjectPaths: targetPaths,
          agents,
          privateCopyAgents,
        });

      const successCount = result.results.filter((r) => r.success).length;
      const failCount = result.results.filter((r) => !r.success).length;
      const skippedAgents = result.results
        .flatMap((r) => r.skippedAgents ?? [])
        .filter((agent, index, agents) => agents.indexOf(agent) === index);

      if (failCount > 0) {
        const errors = result.results
          .filter((r) => !r.success)
          .map((r) => `${r.projectPath}: ${r.error}`)
          .join('\n');
        toast.error(t('skills.copyToProject.partialError', { success: successCount, fail: failCount }) + '\n' + errors);
      } else if (skippedAgents.length > 0) {
        toast.warning(t('skills.copyToProject.skippedAgents', { agents: skippedAgents.join(', ') }));
      } else {
        toast.success(t('skills.copyToProject.success', { count: successCount }));
      }

      set({ copySkill: null, copyContext: undefined });
    } catch (e) {
      console.error('[executeCopy] Failed:', e);
      toast.error(String(e));
    }
  },
}));
