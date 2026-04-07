// src/stores/skill-detail.ts
import { create } from 'zustand';
import { readSkillContent as apiReadSkillContent } from '@/hooks/useTauriApi';
import { useContextStore } from './context';
import {
  findSkillByIdentity,
  getSkillIdentity,
  getSkillIdentityKey,
  type SkillIdentity,
} from '@/lib/skills/identity';
import type { InstalledSkill } from '@/bindings';

interface SkillDetailState {
  selectedSkillRef: SkillIdentity | null;
  skillContent: string | null;
  loadingContent: boolean;

  selectSkill: (skill: InstalledSkill) => Promise<void>;
  deselectSkill: () => void;
  reloadContent: () => Promise<void>;
}

function getSelectedSkillKey(identity: SkillIdentity | null): string | null {
  return identity ? getSkillIdentityKey(identity) : null;
}

function getSelectionIdentity(skill: InstalledSkill): SkillIdentity {
  const projectPath =
    skill.scope === 'project' ? useContextStore.getState().selectedContext : undefined;
  return getSkillIdentity(skill, projectPath);
}

async function resolveSelectedSkill(identity: SkillIdentity): Promise<InstalledSkill | null> {
  const { useSkillsDataStore } = await import('./skills-data');
  const { globalSkills, projectSkills } = useSkillsDataStore.getState();
  return findSkillByIdentity(
    identity,
    globalSkills,
    projectSkills,
    useContextStore.getState().selectedContext
  );
}

export const useSkillDetailStore = create<SkillDetailState>()((set, get) => ({
  selectedSkillRef: null,
  skillContent: null,
  loadingContent: false,

  selectSkill: async (skill) => {
    const nextSelectedSkillRef = getSelectionIdentity(skill);
    const nextSelectedSkillKey = getSelectedSkillKey(nextSelectedSkillRef);

    // js-early-exit: same skill → no-op (use reloadContent for retry)
    if (getSelectedSkillKey(get().selectedSkillRef) === nextSelectedSkillKey) return;

    set({
      selectedSkillRef: nextSelectedSkillRef,
      skillContent: null,
      loadingContent: true,
    });

    try {
      const content = await apiReadSkillContent(skill.canonicalPath);
      // Race condition guard: only apply if still the same skill
      if (getSelectedSkillKey(get().selectedSkillRef) === nextSelectedSkillKey) {
        set({ skillContent: content, loadingContent: false });
      }
    } catch (e) {
      if (getSelectedSkillKey(get().selectedSkillRef) === nextSelectedSkillKey) {
        console.error('[selectSkill] Failed to read content:', e);
        set({ skillContent: null, loadingContent: false });
      }
    }
  },

  deselectSkill: () => set({ selectedSkillRef: null, skillContent: null, loadingContent: false }),

  reloadContent: async () => {
    const { selectedSkillRef } = get();
    if (!selectedSkillRef) return;

    const selectedSkillKey = getSelectedSkillKey(selectedSkillRef);
    set({ skillContent: null, loadingContent: true });

    try {
      const selectedSkill = await resolveSelectedSkill(selectedSkillRef);
      if (!selectedSkill) {
        if (getSelectedSkillKey(get().selectedSkillRef) === selectedSkillKey) {
          set({ skillContent: null, loadingContent: false });
        }
        return;
      }

      const content = await apiReadSkillContent(selectedSkill.canonicalPath);
      if (getSelectedSkillKey(get().selectedSkillRef) === selectedSkillKey) {
        set({ skillContent: content, loadingContent: false });
      }
    } catch {
      if (getSelectedSkillKey(get().selectedSkillRef) === selectedSkillKey) {
        set({ skillContent: null, loadingContent: false });
      }
    }
  },
}));
