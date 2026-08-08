// src/stores/skill-detail.ts
import { create } from 'zustand';
import { readSkillContent as apiReadSkillContent } from '@/hooks/useTauriApi';
import { useWorkspaceContextStore } from './workspace-context';
import { projectSnapshotFor } from './projects';
import { contextKey, globalContext } from '@/lib/context';
import {
  findSkillByIdentity,
  getSkillIdentity,
  getSkillIdentityKey,
  type SkillIdentity,
} from '@/lib/skills/identity';
import type { SkillLocationRef, InstalledSkill } from '@/bindings';

interface SkillDetailState {
  selectedSkillRef: SkillIdentity | null;
  selectedContext: SkillLocationRef | null;
  skillContent: string | null;
  loadingContent: boolean;

  selectSkill: (skill: InstalledSkill) => Promise<void>;
  deselectSkill: () => void;
  reloadContent: () => Promise<void>;
}

function getContextualSelectionKey(
  context: SkillLocationRef | null,
  identity: SkillIdentity | null,
): string | null {
  return context && identity
    ? `${contextKey(context)}:${getSkillIdentityKey(identity)}`
    : null;
}

function projectPathForContext(context: SkillLocationRef): string | undefined {
  const scope = context.scope;
  if (scope.scope !== 'project') return undefined;
  return projectSnapshotFor(context.environment).projects.find(
    (project) => project.binding.id === scope.project_id,
  )?.binding.nativePath;
}

function getSelectionIdentity(skill: InstalledSkill, context: SkillLocationRef): SkillIdentity {
  const projectPath = skill.scope === 'project' ? projectPathForContext(context) : undefined;
  return getSkillIdentity(skill, projectPath);
}

async function resolveSelectedSkill(
  identity: SkillIdentity,
  context: SkillLocationRef,
): Promise<InstalledSkill | null> {
  const { useSkillsDataStore } = await import('./skills-data');
  const snapshots = useSkillsDataStore.getState().snapshots;
  const globalSkills = snapshots[contextKey(globalContext(context.environment))]?.skills ?? [];
  const projectSkills = context.scope.scope === 'project'
    ? snapshots[contextKey(context)]?.skills ?? []
    : [];
  return findSkillByIdentity(
    identity,
    globalSkills,
    projectSkills,
    projectPathForContext(context),
  );
}

export const useSkillDetailStore = create<SkillDetailState>()((set, get) => ({
  selectedSkillRef: null,
  selectedContext: null,
  skillContent: null,
  loadingContent: false,

  selectSkill: async (skill) => {
    const workspaceContext = useWorkspaceContextStore.getState().selectedContext;
    const selectedContext = skill.scope === 'global'
      ? globalContext(workspaceContext.environment)
      : workspaceContext;
    const nextSelectedSkillRef = getSelectionIdentity(skill, selectedContext);
    const nextSelectedSkillKey = getContextualSelectionKey(selectedContext, nextSelectedSkillRef);

    // js-early-exit: same skill → no-op (use reloadContent for retry)
    const currentSelectionKey = getContextualSelectionKey(
      get().selectedContext,
      get().selectedSkillRef,
    );
    if (currentSelectionKey === nextSelectedSkillKey) return;

    set({
      selectedSkillRef: nextSelectedSkillRef,
      selectedContext,
      skillContent: null,
      loadingContent: true,
    });

    try {
      const content = await apiReadSkillContent({
        context: selectedContext,
        skillName: skill.name,
      });
      // Race condition guard: only apply if still the same skill
      if (getContextualSelectionKey(get().selectedContext, get().selectedSkillRef) === nextSelectedSkillKey) {
        set({ skillContent: content, loadingContent: false });
      }
    } catch (e) {
      if (getContextualSelectionKey(get().selectedContext, get().selectedSkillRef) === nextSelectedSkillKey) {
        console.error('[selectSkill] Failed to read content:', e);
        set({ skillContent: null, loadingContent: false });
      }
    }
  },

  deselectSkill: () => set({ selectedSkillRef: null, selectedContext: null, skillContent: null, loadingContent: false }),

  reloadContent: async () => {
    const { selectedSkillRef, selectedContext } = get();
    if (!selectedSkillRef || !selectedContext) return;

    const selectedSkillKey = getContextualSelectionKey(selectedContext, selectedSkillRef);
    set({ skillContent: null, loadingContent: true });

    try {
      const selectedSkill = await resolveSelectedSkill(selectedSkillRef, selectedContext);
      if (!selectedSkill) {
        if (getContextualSelectionKey(get().selectedContext, get().selectedSkillRef) === selectedSkillKey) {
          set({ skillContent: null, loadingContent: false });
        }
        return;
      }

      const content = await apiReadSkillContent({
        context: selectedContext,
        skillName: selectedSkill.name,
      });
      if (getContextualSelectionKey(get().selectedContext, get().selectedSkillRef) === selectedSkillKey) {
        set({ skillContent: content, loadingContent: false });
      }
    } catch {
      if (getContextualSelectionKey(get().selectedContext, get().selectedSkillRef) === selectedSkillKey) {
        set({ skillContent: null, loadingContent: false });
      }
    }
  },
}));
