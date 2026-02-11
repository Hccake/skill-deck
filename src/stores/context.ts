// src/stores/context.ts
import { create } from 'zustand';
import {
  getConfig,
  addProject as apiAddProject,
  removeProject as apiRemoveProject,
} from '@/hooks/useTauriApi';

interface ContextState {
  /** 当前选中的 context: 'global' 或项目路径（运行时状态，不持久化） */
  selectedContext: string;
  /** 从后端加载的项目列表缓存 */
  projects: string[];
  /** 项目列表是否已加载 */
  projectsLoaded: boolean;

  /** 从后端加载 projects */
  loadProjects: () => Promise<void>;
  /** 添加项目（调用后端 + 更新缓存） */
  addProject: (path: string) => Promise<void>;
  /** 移除项目（调用后端 + 更新缓存） */
  removeProject: (path: string) => Promise<void>;

  /** 选择 context */
  selectContext: (ctx: string) => void;
  /** 切换项目选中状态（支持取消选中） */
  toggleProjectContext: (path: string) => void;
}

export const useContextStore = create<ContextState>()((set, get) => ({
  selectedContext: 'global',
  projects: [],
  projectsLoaded: false,

  loadProjects: async () => {
    try {
      const config = await getConfig();
      set({ projects: config.projects, projectsLoaded: true });
    } catch (error) {
      console.error('Failed to load projects:', error);
      set({ projects: [], projectsLoaded: true });
    }
  },

  addProject: async (path) => {
    try {
      const updatedProjects = await apiAddProject(path);
      set({ projects: updatedProjects });
    } catch (error) {
      console.error('Failed to add project:', error);
    }
  },

  removeProject: async (path) => {
    try {
      const updatedProjects = await apiRemoveProject(path);
      // 如果删除的是当前选中的项目，回到 global
      const { selectedContext } = get();
      if (selectedContext === path) {
        set({ projects: updatedProjects, selectedContext: 'global' });
      } else {
        set({ projects: updatedProjects });
      }
    } catch (error) {
      console.error('Failed to remove project:', error);
    }
  },

  selectContext: (ctx) => {
    set({ selectedContext: ctx });
  },

  toggleProjectContext: (path) => {
    const current = get().selectedContext;
    if (current === path) {
      set({ selectedContext: 'global' });
    } else {
      set({ selectedContext: path });
    }
  },
}));
