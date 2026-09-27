import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface LayoutState {
  skillsListWidth: number;
  libraryListWidth: number;
  libraryDetailOpen: boolean;
  setLibraryDetailOpen: (open: boolean) => void;
  setSkillsListWidth: (width: number) => void;
  setLibraryListWidth: (width: number) => void;
}

function validWidth(width: unknown): width is number {
  return typeof width === 'number' && Number.isFinite(width) && width >= 240 && width <= 4096;
}

export const useLayoutStore = create<LayoutState>()(persist(
  (set) => ({
    skillsListWidth: 320,
    libraryListWidth: 320,
    libraryDetailOpen: false,
    setLibraryDetailOpen: (libraryDetailOpen) => set({ libraryDetailOpen }),
    setSkillsListWidth: (width) => {
      if (validWidth(width)) set({ skillsListWidth: Math.round(width) });
    },
    setLibraryListWidth: (width) => {
      if (validWidth(width)) set({ libraryListWidth: Math.round(width) });
    },
  }),
  {
    name: 'skill-deck-layout',
    version: 1,
    partialize: ({ skillsListWidth, libraryListWidth }) => ({ skillsListWidth, libraryListWidth }),
    merge: (persisted, current) => {
      const saved = persisted as Partial<LayoutState> | null;
      return {
        ...current,
        skillsListWidth: validWidth(saved?.skillsListWidth) ? saved.skillsListWidth : 320,
        libraryListWidth: validWidth(saved?.libraryListWidth) ? saved.libraryListWidth : 320,
      };
    },
  },
));
