// src/stores/skill-dialog.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import {
  createSkillRepairDraft,
  t,
  type AddDialogPrefill,
  type DeleteTarget,
  type RepairSourceDraft,
} from './skills-utils';
import {
  openInstallWizard,
} from '@/hooks/useTauriApi';
import { projectSnapshotFor } from './projects';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import type {
  InstalledSkill,
  SkillScope,
  ContextRef,
  ManageAgentSelectionSnapshot,
  RemovePreview,
} from '@/bindings';
import { formatWorkflowError } from '@/workflows/mutation-presentation';

interface SkillDialogState {
  // Delete dialog
  deleteTarget: DeleteTarget | null;
  deletePreview: RemovePreview | null;
  deleteFeedback: 'previewError' | 'executionError' | 'stale' | null;
  loadingAgentDetails: boolean;

  // Manage agents dialog
  manageAgentsSkill: InstalledSkill | null;
  manageAgentsScope: SkillScope;
  manageAgentsProjectPath?: string;
  manageAgentsContext?: ContextRef;
  manageAgentDetails: ManageAgentSelectionSnapshot | null;
  loadingManageAgentDetails: boolean;

  // Copy to project dialog
  copySkill: InstalledSkill | null;
  copyContext?: ContextRef;

  // Repair source dialog
  repairSourceTarget: RepairSourceDraft | null;

  // Actions
  openDelete: (skill: InstalledSkill, context: ContextRef, projectPath?: string) => void;
  setDeletePreview: (preview: RemovePreview | null) => void;
  setDeleteFeedback: (feedback: SkillDialogState['deleteFeedback']) => void;
  setDeleteLoading: (loading: boolean) => void;
  closeDelete: () => void;
  openAdd: (context: ContextRef, projectPath?: string) => void;
  openAddWithPrefill: (prefill: AddDialogPrefill, context: ContextRef) => void;
  openRepairSource: (
    skill: InstalledSkill,
    context: ContextRef,
    projectPath?: string,
  ) => void;
  closeRepairSource: () => void;
  openManageAgents: (skill: InstalledSkill, context: ContextRef, projectPath?: string) => void;
  setManageAgentDetails: (snapshot: ManageAgentSelectionSnapshot | null) => void;
  setManageAgentLoading: (loading: boolean) => void;
  closeManageAgents: () => void;
  openCopyToProject: (skill: InstalledSkill, context: ContextRef) => void;
  closeCopyToProject: () => void;
}

function projectPathForContext(context: ContextRef): string | undefined {
  const scope = context.scope;
  if (scope.scope !== 'project') return undefined;
  return projectSnapshotFor(context.environment).projects.find(
    (project) => project.binding.id === scope.project_id,
  )?.binding.nativePath;
}

export const useSkillDialogStore = create<SkillDialogState>()((set) => ({
  deleteTarget: null,
  deletePreview: null,
  deleteFeedback: null,
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
      deletePreview: null,
      deleteFeedback: null,
      loadingAgentDetails: true,
    });
  },

  setDeletePreview: (deletePreview) => set({ deletePreview }),

  setDeleteFeedback: (deleteFeedback) => set({ deleteFeedback }),

  setDeleteLoading: (loadingAgentDetails) => set({ loadingAgentDetails }),

  closeDelete: () => set({
    deleteTarget: null,
    deletePreview: null,
    deleteFeedback: null,
    loadingAgentDetails: false,
  }),

  openAdd: (context, projectPath = projectPathForContext(context)) => {
    if (isBusinessWriteBlocked()) return;
    const scope = context.scope.scope;
    openInstallWizard({
      entryPoint: 'skills-panel',
      projectPath: scope === 'project' ? projectPath : undefined,
      context,
    }).catch((e) => {
      console.error('[openAdd] Failed to open wizard:', e);
      toast.error(formatWorkflowError(e, t));
    });
  },

  openAddWithPrefill: (prefill, context) => {
    if (isBusinessWriteBlocked()) return;
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
      toast.error(formatWorkflowError(e, t));
    });
  },

  openRepairSource: (skill, context, projectPath = projectPathForContext(context)) => {
    const repairSourceTarget = createSkillRepairDraft(
      skill,
      context,
      projectPath,
    );
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
  },

  setManageAgentDetails: (manageAgentDetails) => set({ manageAgentDetails }),

  setManageAgentLoading: (loadingManageAgentDetails) => set({ loadingManageAgentDetails }),

  closeManageAgents: () => set({
    manageAgentsSkill: null,
    manageAgentsProjectPath: undefined,
    manageAgentsContext: undefined,
    manageAgentDetails: null,
    loadingManageAgentDetails: false,
  }),

  openCopyToProject: (skill, context) => {
    set({
      copySkill: skill,
      copyContext: context,
    });
  },

  closeCopyToProject: () => set({ copySkill: null, copyContext: undefined }),
}));
