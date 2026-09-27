import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { SKILLS_SIDEBAR_DEFAULT_WIDTH, SKILLS_SIDEBAR_MIN_WIDTH, SKILLS_SIDEBAR_MAX_WIDTH } from '@/lib/skills-layout';

interface LayoutState {
  skillsListWidth: number | null;
  libraryListWidth: number | null;
  skillsSidebarWidth: number;
  librarySidebarWidth: number;
  libraryDetailOpen: boolean;
  setLibraryDetailOpen: (open: boolean) => void;
  setSkillsListWidth: (width: number) => void;
  setLibraryListWidth: (width: number) => void;
  setSkillsSidebarWidth: (width: number) => void;
  setLibrarySidebarWidth: (width: number) => void;
}

function validWidth(width: unknown): width is number {
  return typeof width === 'number' && Number.isFinite(width) && width >= 240 && width <= 4096;
}

function validSidebarWidth(width: unknown): width is number {
  return typeof width === 'number' && Number.isFinite(width) && width >= SKILLS_SIDEBAR_MIN_WIDTH && width <= SKILLS_SIDEBAR_MAX_WIDTH;
}

export const useLayoutStore = create<LayoutState>()(persist(
  (set) => ({
    skillsListWidth: null,
    libraryListWidth: null,
    skillsSidebarWidth: SKILLS_SIDEBAR_DEFAULT_WIDTH,
    librarySidebarWidth: SKILLS_SIDEBAR_DEFAULT_WIDTH,
    libraryDetailOpen: false,
    setLibraryDetailOpen: (libraryDetailOpen) => set({ libraryDetailOpen }),
    setSkillsListWidth: (width) => {
      if (validWidth(width)) set({ skillsListWidth: Math.round(width) });
    },
    setLibraryListWidth: (width) => {
      if (validWidth(width)) set({ libraryListWidth: Math.round(width) });
    },
    setSkillsSidebarWidth: (width) => {
      if (validSidebarWidth(width)) set({ skillsSidebarWidth: Math.round(width) });
    },
    setLibrarySidebarWidth: (width) => {
      if (validSidebarWidth(width)) set({ librarySidebarWidth: Math.round(width) });
    },
  }),
  {
    name: 'skill-deck-layout',
    version: 2,
    partialize: ({ skillsListWidth, libraryListWidth, skillsSidebarWidth, librarySidebarWidth }) => ({ skillsListWidth, libraryListWidth, skillsSidebarWidth, librarySidebarWidth }),
    merge: (persisted, current) => {
      const saved = persisted as Partial<LayoutState> | null;
      return {
        ...current,
        skillsListWidth: validWidth(saved?.skillsListWidth) ? saved.skillsListWidth : null,
        libraryListWidth: validWidth(saved?.libraryListWidth) ? saved.libraryListWidth : null,
        skillsSidebarWidth: validSidebarWidth(saved?.skillsSidebarWidth) ? saved.skillsSidebarWidth : SKILLS_SIDEBAR_DEFAULT_WIDTH,
        librarySidebarWidth: validSidebarWidth(saved?.librarySidebarWidth) ? saved.librarySidebarWidth : SKILLS_SIDEBAR_DEFAULT_WIDTH,
      };
    },
  },
));
