// src/stores/skill-detail.ts
import { create } from 'zustand';
import { readSkillContent as apiReadSkillContent } from '@/hooks/useTauriApi';
import type { InstalledSkill } from '@/bindings';

interface SkillDetailState {
  selectedSkill: InstalledSkill | null;
  skillContent: string | null;
  loadingContent: boolean;

  selectSkill: (skill: InstalledSkill) => Promise<void>;
  deselectSkill: () => void;
  reloadContent: () => Promise<void>;
}

export const useSkillDetailStore = create<SkillDetailState>()((set, get) => ({
  selectedSkill: null,
  skillContent: null,
  loadingContent: false,

  selectSkill: async (skill) => {
    // js-early-exit: same skill → no-op (use reloadContent for retry)
    if (get().selectedSkill?.name === skill.name && get().selectedSkill?.scope === skill.scope) return;

    set({ selectedSkill: skill, skillContent: null, loadingContent: true });

    try {
      const content = await apiReadSkillContent(skill.canonicalPath);
      // Race condition guard: only apply if still the same skill
      if (get().selectedSkill?.name === skill.name && get().selectedSkill?.scope === skill.scope) {
        set({ skillContent: content, loadingContent: false });
      }
    } catch (e) {
      if (get().selectedSkill?.name === skill.name && get().selectedSkill?.scope === skill.scope) {
        console.error('[selectSkill] Failed to read content:', e);
        set({ skillContent: null, loadingContent: false });
      }
    }
  },

  deselectSkill: () => set({ selectedSkill: null, skillContent: null, loadingContent: false }),

  reloadContent: async () => {
    const { selectedSkill } = get();
    if (!selectedSkill) return;
    set({ skillContent: null, loadingContent: true });
    try {
      const content = await apiReadSkillContent(selectedSkill.canonicalPath);
      if (get().selectedSkill?.name === selectedSkill.name) {
        set({ skillContent: content, loadingContent: false });
      }
    } catch {
      if (get().selectedSkill?.name === selectedSkill.name) {
        set({ skillContent: null, loadingContent: false });
      }
    }
  },
}));
