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
  SkillLocationRef,
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
  manageAgentsContext?: SkillLocationRef;

  // Copy to project dialog
  copySkill: InstalledSkill | null;
  copyContext?: SkillLocationRef;

  // Repair source dialog
  repairSourceTarget: RepairSourceDraft | null;

  // Actions
  openDelete: (skill: InstalledSkill, context: SkillLocationRef, projectPath?: string) => void;
  setDeletePreview: (preview: RemovePreview | null) => void;
  setDeleteFeedback: (feedback: SkillDialogState['deleteFeedback']) => void;
  setDeleteLoading: (loading: boolean) => void;
  closeDelete: () => void;
  openAdd: (context: SkillLocationRef, projectPath?: string) => void;
  openAddWithPrefill: (prefill: AddDialogPrefill, context: SkillLocationRef) => void;
  openRepairSource: (
    skill: InstalledSkill,
    context: SkillLocationRef,
    projectPath?: string,
  ) => void;
  closeRepairSource: () => void;
  openManageAgents: (skill: InstalledSkill, context: SkillLocationRef) => void;
  closeManageAgents: () => void;
  openCopyToProject: (skill: InstalledSkill, context: SkillLocationRef) => void;
  closeCopyToProject: () => void;
}

function projectPathForContext(context: SkillLocationRef): string | undefined {
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
  manageAgentsContext: undefined,
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

  openManageAgents: (skill, context) => {
    set({
      manageAgentsSkill: skill,
      manageAgentsContext: context,
    });
  },

  closeManageAgents: () => set({
    manageAgentsSkill: null,
    manageAgentsContext: undefined,
  }),

  openCopyToProject: (skill, context) => {
    set({
      copySkill: skill,
      copyContext: context,
    });
  },

  closeCopyToProject: () => set({ copySkill: null, copyContext: undefined }),
}));
