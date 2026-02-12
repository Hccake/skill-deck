// src/stores/skills.ts
import { create } from 'zustand';
import { toast } from 'sonner';
import i18n from '@/i18n';
import { useContextStore } from './context';
import {
  listSkills,
  listAgents,
  removeSkill as apiRemoveSkill,
  checkUpdates,
  updateSkill as apiUpdateSkill,
  openInstallWizard,
} from '@/hooks/useTauriApi';
import type { AgentInfo, InstalledSkill, SkillScope, SkillUpdateInfo } from '@/bindings';

/** 按名称排序 skills，保证展示顺序稳定 */
function sortSkills(skills: InstalledSkill[]): InstalledSkill[] {
  return [...skills].sort((a, b) => a.name.localeCompare(b.name));
}

/** 将 check_updates 结果合并到 skills 列表 */
function mergeUpdateInfo(skills: InstalledSkill[], updates: SkillUpdateInfo[]): InstalledSkill[] {
  const updateMap = new Map(updates.map((u) => [u.name, u.hasUpdate]));
  return skills.map((s) => ({
    ...s,
    hasUpdate: updateMap.get(s.name) ?? false,
  }));
}

/** i18n t() 的便捷包装 */
function t(key: string, options?: Record<string, unknown>): string {
  return i18n.t(key, options);
}

export interface DeleteTarget {
  name: string;
  scope: SkillScope;
  projectPath?: string;
}

export interface AddDialogPrefill {
  source: string;
  skillName: string;
}

interface SkillsState {
  // 数据层
  globalSkills: InstalledSkill[];
  projectSkills: InstalledSkill[];
  projectPathExists: boolean;
  allAgents: AgentInfo[];
  loading: boolean;
  error: string | null;

  // 操作层
  isSyncing: boolean;
  updatingSkill: string | null;

  // Dialog 触发状态
  detailSkill: InstalledSkill | null;
  deleteTarget: DeleteTarget | null;

  // Actions — 内部通过 useContextStore.getState() 获取 selectedContext
  fetchSkills: () => Promise<void>;
  syncSkills: () => Promise<void>;
  updateSkill: (skillName: string) => Promise<void>;
  deleteSkill: () => Promise<void>;
  openDetail: (skill: InstalledSkill) => void;
  closeDetail: () => void;
  openDelete: (name: string, scope: SkillScope, projectPath?: string) => void;
  closeDelete: () => void;
  openAdd: (scope: SkillScope) => void;
  openAddWithPrefill: (prefill: AddDialogPrefill) => void;
}

export const useSkillsStore = create<SkillsState>()((set, get) => ({
  // 数据层初始值
  globalSkills: [],
  projectSkills: [],
  projectPathExists: true,
  allAgents: [],
  loading: true,
  error: null,

  // 操作层初始值
  isSyncing: false,
  updatingSkill: null,

  // Dialog 初始值
  detailSkill: null,
  deleteTarget: null,

  // === Actions ===

  fetchSkills: async () => {
    const { selectedContext } = useContextStore.getState();
    const isProjectSelected = selectedContext !== 'global';

    try {
      set({ loading: true, error: null });

      if (isProjectSelected) {
        const [agents, globalResult, projectResult] = await Promise.all([
          listAgents(),
          listSkills({ scope: 'global' }),
          listSkills({ scope: 'project', projectPath: selectedContext }),
        ]);
        set({
          allAgents: agents,
          globalSkills: sortSkills(globalResult.skills),
          projectSkills: sortSkills(projectResult.skills),
          projectPathExists: projectResult.pathExists,
        });
      } else {
        const [agents, globalResult] = await Promise.all([
          listAgents(),
          listSkills({ scope: 'global' }),
        ]);
        set({
          allAgents: agents,
          globalSkills: sortSkills(globalResult.skills),
          projectSkills: [],
          projectPathExists: true,
        });
      }
    } catch (e) {
      set({ error: e instanceof Error ? e.message : 'Failed to load skills' });
    } finally {
      set({ loading: false });
    }
  },

  syncSkills: async () => {
    const { selectedContext } = useContextStore.getState();
    const isProjectSelected = selectedContext !== 'global';

    set({ isSyncing: true });
    try {
      if (isProjectSelected) {
        const [globalResult, projectResult, globalUpdates, projectUpdates] =
          await Promise.all([
            listSkills({ scope: 'global' }),
            listSkills({ scope: 'project', projectPath: selectedContext }),
            checkUpdates('global').catch(() => [] as SkillUpdateInfo[]),
            checkUpdates('project', selectedContext).catch(() => [] as SkillUpdateInfo[]),
          ]);

        set({
          globalSkills: sortSkills(mergeUpdateInfo(globalResult.skills, globalUpdates)),
          projectSkills: sortSkills(mergeUpdateInfo(projectResult.skills, projectUpdates)),
          projectPathExists: projectResult.pathExists,
        });
      } else {
        const [globalResult, globalUpdates] = await Promise.all([
          listSkills({ scope: 'global' }),
          checkUpdates('global').catch(() => [] as SkillUpdateInfo[]),
        ]);

        set({
          globalSkills: sortSkills(mergeUpdateInfo(globalResult.skills, globalUpdates)),
          projectSkills: [],
          projectPathExists: true,
        });
      }
    } catch (e) {
      set({ error: e instanceof Error ? e.message : 'Failed to sync skills' });
    } finally {
      set({ isSyncing: false });
    }
  },

  updateSkill: async (skillName: string) => {
    if (get().updatingSkill) return;

    const { selectedContext } = useContextStore.getState();
    const isProjectSkill = get().projectSkills.some((s) => s.name === skillName);
    const scope: SkillScope = isProjectSkill ? 'project' : 'global';

    set({ updatingSkill: skillName });
    try {
      await apiUpdateSkill({
        scope,
        name: skillName,
        projectPath: isProjectSkill ? selectedContext : undefined,
      });
      toast.success(t('skills.updateSuccess', { name: skillName }));
      await get().syncSkills();
    } catch (e) {
      toast.error(
        t('skills.updateError', {
          name: skillName,
          error: e instanceof Error ? e.message : String(e),
        })
      );
    } finally {
      set({ updatingSkill: null });
    }
  },

  deleteSkill: async () => {
    const { deleteTarget } = get();
    if (!deleteTarget) return;

    try {
      await apiRemoveSkill({
        scope: deleteTarget.scope,
        name: deleteTarget.name,
        projectPath: deleteTarget.projectPath,
      });
      toast.success(t('skills.deleteSuccess', { name: deleteTarget.name }));
      set({ deleteTarget: null });
      await get().fetchSkills();
    } catch (e) {
      toast.error(t('skills.deleteError', {
        name: deleteTarget.name,
        error: e instanceof Error ? e.message : String(e),
      }));
      set({ deleteTarget: null });
    }
  },

  // Dialog actions
  openDetail: (skill) => set({ detailSkill: skill }),
  closeDetail: () => set({ detailSkill: null }),

  openDelete: (name, scope, projectPath) =>
    set({ deleteTarget: { name, scope, projectPath } }),
  closeDelete: () => set({ deleteTarget: null }),

  openAdd: (scope) => {
    const { selectedContext } = useContextStore.getState();
    openInstallWizard({
      entryPoint: 'skills-panel',
      scope,
      projectPath: scope === 'project' ? selectedContext : undefined,
    }).catch((e) => {
      console.error('[openAdd] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },
  openAddWithPrefill: (prefill) => {
    openInstallWizard({
      entryPoint: 'discovery',
      scope: 'global',
      prefillSource: prefill.source,
      prefillSkillName: prefill.skillName,
    }).catch((e) => {
      console.error('[openAddWithPrefill] Failed to open wizard:', e);
      toast.error(String(e));
    });
  },
}));
