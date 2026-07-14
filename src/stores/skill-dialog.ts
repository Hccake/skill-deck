// src/stores/skill-dialog.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import {
  createSkillRepairDraft,
  getSkillOperationAgents,
  t,
  type AddDialogPrefill,
  type DeleteTarget,
  type RepairSourceDraft,
} from './skills-utils';
import { getSkillIdentity, isSameSkillIdentity } from '@/lib/skills/identity';
import { appendCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import {
  removeSkill as apiRemoveSkill,
  getSkillAgentDetails as apiGetAgentDetails,
  openInstallWizard,
  manageSkillAgents as apiManageSkillAgents,
  cleanupDuplicateAgentCopies as apiCleanupDuplicateAgentCopies,
  copySkillToProjects as apiCopySkillToProjects,
} from '@/hooks/useTauriApi';
import { useProjectStore } from './projects';
import { environmentKey } from '@/lib/context';
import { isMutationWriteBlocked } from './mutation';
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
  openDelete: (skill: InstalledSkill, context: ContextRef, projectPath?: string) => void;
  closeDelete: () => void;
  deleteSkill: (params: {
    fullRemoval: boolean;
    agents?: AgentType[];
    agentTargets?: InstallTargetSpec[];
  }) => Promise<void>;
  openAdd: (context: ContextRef, projectPath?: string) => void;
  openAddWithPrefill: (prefill: AddDialogPrefill, context: ContextRef) => void;
  openRepairSource: (
    skill: InstalledSkill,
    context: ContextRef,
    projectPath?: string,
  ) => void;
  closeRepairSource: () => void;
  openManageAgents: (skill: InstalledSkill, context: ContextRef, projectPath?: string) => void;
  closeManageAgents: () => void;
  saveAgentChanges: (
    addAgents: string[],
    removeAgents: string[],
    mode: InstallMode,
    privateCopyAgents?: string[],
    removePrivateCopyAgents?: string[],
  ) => Promise<void>;
  cleanupDuplicateCopies: (agents: string[]) => Promise<void>;
  openCopyToProject: (skill: InstalledSkill, context: ContextRef) => void;
  closeCopyToProject: () => void;
  executeCopy: (targetPaths: string[]) => Promise<void>;
}

function projectPathForContext(context: ContextRef): string | undefined {
  const scope = context.scope;
  if (scope.scope !== 'project') return undefined;
  const projects = useProjectStore.getState().projectsByEnvironment[
    environmentKey(context.environment)
  ] ?? [];
  return projects.find((project) => project.binding.id === scope.project_id)?.binding.nativePath;
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

  openDelete: (skill, context, projectPath = projectPathForContext(context)) => {
    const scope = context.scope.scope;
    set({
      deleteTarget: { skill, scope, projectPath, context },
      agentDetails: null,
      loadingAgentDetails: true,
    });
    apiGetAgentDetails(context, skill.name)
      .then((details) => set({ agentDetails: details }))
      .catch((e) => console.warn('Failed to fetch agent details:', e))
      .finally(() => set({ loadingAgentDetails: false }));
  },

  closeDelete: () => set({ deleteTarget: null, agentDetails: null, loadingAgentDetails: false }),

  deleteSkill: async ({ fullRemoval, agents, agentTargets }) => {
    if (isMutationWriteBlocked()) return;
    const { deleteTarget } = get();
    if (!deleteTarget) return;

    try {
      const context = deleteTarget.context;
      await apiRemoveSkill(context, {
        name: deleteTarget.skill.name,
        fullRemoval,
        agents,
        agentTargets,
      });
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
      await useSkillsDataStore.getState().syncSkills(context);
    } catch (e) {
      toast.error(appendCrossStorageFailureGuidance(
        t('skills.deleteError', {
          name: deleteTarget.skill.name,
          error: e instanceof Error ? e.message : String(e),
        }),
        deleteTarget.context,
        'delete',
        t,
      ));
      set({ deleteTarget: null, agentDetails: null });
    }
  },

  openAdd: (context, projectPath = projectPathForContext(context)) => {
    if (isMutationWriteBlocked()) return;
    const scope = context.scope.scope;
    openInstallWizard({
      entryPoint: 'skills-panel',
      projectPath: scope === 'project' ? projectPath : undefined,
      context,
    }).catch((e) => {
      console.error('[openAdd] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },

  openAddWithPrefill: (prefill, context) => {
    if (isMutationWriteBlocked()) return;
    const scope = prefill.scope ?? context.scope.scope;
    const projectPath =
      scope === 'project'
        ? prefill.projectPath ?? projectPathForContext(context)
        : undefined;
    openInstallWizard({
      entryPoint: 'discovery',
      projectPath,
      prefillSource: prefill.source,
      prefillSkillName: prefill.skillName,
      context,
    }).catch((e) => {
      console.error('[openAddWithPrefill] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },

  openRepairSource: (skill, context, projectPath = projectPathForContext(context)) => {
    const repairSourceTarget = createSkillRepairDraft(
      skill,
      context,
      projectPath,
    );
    if (!repairSourceTarget) return;
    set({ repairSourceTarget });
  },

  closeRepairSource: () => set({ repairSourceTarget: null }),

  openManageAgents: (skill, context, projectPath = projectPathForContext(context)) => {
    const scope = context.scope.scope;
    const manageAgentsProjectPath = scope === 'project' ? projectPath : undefined;
    set({
      manageAgentsSkill: skill,
      manageAgentsScope: scope,
      manageAgentsProjectPath,
      manageAgentsContext: context,
      manageAgentDetails: null,
      loadingManageAgentDetails: true,
    });
    apiGetAgentDetails(context, skill.name)
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
    if (isMutationWriteBlocked()) return;
    const { manageAgentsSkill, manageAgentsContext } = get();
    if (!manageAgentsSkill) return;

    const context = manageAgentsContext;
    if (!context) return;

    try {
      if (removePrivateCopyAgents.length > 0) {
        const cleanupResults = await apiCleanupDuplicateAgentCopies(context, {
          skillName: manageAgentsSkill.name,
          agents: removePrivateCopyAgents as AgentType[],
        });
        const cleanupFailures = cleanupResults.filter((result) => !result.success && !result.skipped);
        if (cleanupFailures.length > 0) {
          toast.error(appendCrossStorageFailureGuidance(
            cleanupFailures.map((result) => `${result.agent}: ${result.error}`).join('\n'),
            context,
            'cleanup',
            t,
          ));
          return;
        }
      }

      const result = await apiManageSkillAgents(context, {
        skillName: manageAgentsSkill.name,
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
        toast.error(appendCrossStorageFailureGuidance(
          errors.join('\n'),
          context,
          'manageAgents',
          t,
        ));
      } else {
        toast.success(t('skills.manageAgents.success'));
      }

      set({ manageAgentsSkill: null, manageAgentsProjectPath: undefined, manageAgentsContext: undefined });

      // Refresh skills list and detail panel
      const { useSkillsDataStore } = await import('./skills-data');
      await useSkillsDataStore.getState().syncSkills(context);
    } catch (e) {
      console.error('[saveAgentChanges] Failed:', e);
      toast.error(appendCrossStorageFailureGuidance(
        e instanceof Error ? e.message : String(e),
        context,
        'manageAgents',
        t,
      ));
    }
  },

  cleanupDuplicateCopies: async (agents) => {
    if (isMutationWriteBlocked()) return;
    const { manageAgentsSkill, manageAgentsContext } = get();
    if (!manageAgentsSkill || agents.length === 0) return;

    const context = manageAgentsContext;
    if (!context) return;

    try {
      const results = await apiCleanupDuplicateAgentCopies(context, {
        skillName: manageAgentsSkill.name,
        agents: agents as AgentType[],
      });
      const failures = results.filter((result) => !result.success && !result.skipped);
      if (failures.length > 0) {
        toast.error(appendCrossStorageFailureGuidance(
          failures.map((result) => `${result.agent}: ${result.error}`).join('\n'),
          context,
          'cleanup',
          t,
        ));
      } else {
        toast.success(t('skills.manageAgents.cleanupSuccess'));
      }

      const [details] = await Promise.all([
        apiGetAgentDetails(context, manageAgentsSkill.name),
        import('./skills-data').then(({ useSkillsDataStore }) =>
          useSkillsDataStore.getState().syncSkills(context)
        ),
      ]);
      set({ manageAgentDetails: details });
    } catch (e) {
      console.error('[cleanupDuplicateCopies] Failed:', e);
      toast.error(appendCrossStorageFailureGuidance(
        e instanceof Error ? e.message : String(e),
        context,
        'cleanup',
        t,
      ));
    }
  },

  openCopyToProject: (skill, context) => {
    set({
      copySkill: skill,
      copyContext: context,
    });
  },

  closeCopyToProject: () => set({ copySkill: null, copyContext: undefined }),

  executeCopy: async (targetPaths) => {
    if (isMutationWriteBlocked()) return;
    const { copySkill, copyContext } = get();
    if (!copySkill || !copyContext) return;
    const context = copyContext;
    const targetContextsByPath = new Map<string, ContextRef>();

    try {
      const agents = copySkill.privateAdaptedAgents ?? getSkillOperationAgents(copySkill);
      const privateCopyAgents = copySkill.privateCopyAgents ?? [];
      const result = await (async () => {
        const { projectsByEnvironment } = useProjectStore.getState();
        const projects = projectsByEnvironment[environmentKey(context.environment)] ?? [];
        const byPath = new Map(projects.map((project) => [project.binding.nativePath, project]));
        const targets = targetPaths.map((path) => byPath.get(path));
        if (context.scope.scope !== 'project' || targets.some((project) => !project)) {
          throw new Error('Selected projects are not available in the current environment');
        }
        const targetContexts = targets.map((project) => ({
          environment: context.environment,
          scope: { scope: 'project' as const, project_id: project!.binding.id },
        }));
        targets.forEach((project, index) => {
          targetContextsByPath.set(project!.binding.nativePath, targetContexts[index]);
        });
        return apiCopySkillToProjects({
          skillName: copySkill.name,
          source: context,
          targets: targetContexts,
          agents,
          privateCopyAgents,
        });
      })();

      const successCount = result.results.filter((r) => r.success).length;
      const failCount = result.results.filter((r) => !r.success).length;
      const skippedAgents = result.results
        .flatMap((r) => r.skippedAgents ?? [])
        .filter((agent, index, agents) => agents.indexOf(agent) === index);

      if (failCount > 0) {
        const errors = result.results
          .filter((r) => !r.success)
          .map((r) => appendCrossStorageFailureGuidance(
            `${r.projectPath}: ${r.error}`,
            targetContextsByPath.get(r.projectPath) ?? context,
            'copy',
            t,
          ))
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
      toast.error(appendCrossStorageFailureGuidance(
        e instanceof Error ? e.message : String(e),
        context,
        'copy',
        t,
      ));
    }
  },
}));
