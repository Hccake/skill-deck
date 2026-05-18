import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import i18n from '@/i18n';
import {
  getDefaultTargetAgents,
  getLastSelectedAgents,
  listAgents,
  saveDefaultTargetAgents,
} from '@/hooks/useTauriApi';
import type { AgentInfo, DefaultTargetAgents } from '@/hooks/useTauriApi';
import {
  EMPTY_DEFAULT_TARGET_AGENTS,
  filterAdditionalAgentIds,
  migrateDefaultTargetAgents,
  type InstallScope,
} from '@/lib/agentTargets';

export type Theme = 'light' | 'dark';
export type Locale = 'en' | 'zh-CN';

// CLI 默认选中的 agents（与 vercel-skills CLI 一致）
const DEFAULT_AGENTS: string[] = ['claude-code', 'opencode', 'codex'];

interface SettingsState {
  // 主题和语言（保持 localStorage 持久化）
  theme: Theme;
  locale: Locale;
  setTheme: (theme: Theme) => void;
  setLocale: (locale: Locale) => void;
  toggleTheme: () => void;

  // 默认安装目标（读写 ~/.agents/.skill-lock.json）
  allAgents: AgentInfo[];
  defaultTargetAgents: DefaultTargetAgents;
  agentsLoaded: boolean;
  loadDefaultTargetAgents: () => Promise<void>;
  setDefaultTargetAgents: (scope: InstallScope, agents: string[]) => void;
  toggleDefaultTargetAgent: (scope: InstallScope, agentId: string) => void;
  isDefaultTargetAgentSelected: (scope: InstallScope, agentId: string) => boolean;
}

const applyTheme = (theme: Theme) => {
  if (typeof window === 'undefined') return;

  const root = document.documentElement;
  root.classList.remove('light', 'dark');
  root.classList.add(theme);
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set, get) => ({
      // ========== 主题和语言 ==========
      theme: 'light',
      locale: 'en',

      setTheme: (theme) => {
        applyTheme(theme);
        set({ theme });
      },

      setLocale: (locale) => {
        i18n.changeLanguage(locale);
        set({ locale });
      },

      toggleTheme: () => {
        const current = get().theme;
        const next: Theme = current === 'light' ? 'dark' : 'light';
        get().setTheme(next);
      },

      // ========== 默认安装目标 ==========
      allAgents: [],
      defaultTargetAgents: EMPTY_DEFAULT_TARGET_AGENTS,
      agentsLoaded: false,

      loadDefaultTargetAgents: async () => {
        try {
          const agents = await listAgents();
          const [targetDefaultsResult, lastSelectedResult] = await Promise.allSettled([
            getDefaultTargetAgents(),
            getLastSelectedAgents(),
          ]);

          const targetDefaults = targetDefaultsResult.status === 'fulfilled'
            ? targetDefaultsResult.value
            : null;
          const lastSelected = lastSelectedResult.status === 'fulfilled'
            ? lastSelectedResult.value
            : [];

          const migratedDefaults = lastSelected.length > 0
            ? migrateDefaultTargetAgents(lastSelected, agents)
            : migrateDefaultTargetAgents(DEFAULT_AGENTS, agents);

          const defaultTargetAgents = targetDefaults
            ? {
                global: filterAdditionalAgentIds(targetDefaults.global, agents, 'global'),
                project: filterAdditionalAgentIds(targetDefaults.project, agents, 'project'),
              }
            : migratedDefaults;

          set({
            allAgents: agents,
            defaultTargetAgents,
            agentsLoaded: true,
          });
        } catch (error) {
          console.error('加载默认 agents 失败:', error);

          try {
            const agents = await listAgents();
            const defaultTargetAgents = migrateDefaultTargetAgents(DEFAULT_AGENTS, agents);
            set({
              allAgents: agents,
              defaultTargetAgents,
              agentsLoaded: true,
            });
          } catch {
            set({ agentsLoaded: true });
          }
        }
      },

      setDefaultTargetAgents: (scope, agents) => {
        const { allAgents, defaultTargetAgents } = get();
        const nextDefaults = {
          ...defaultTargetAgents,
          [scope]: filterAdditionalAgentIds(agents, allAgents, scope),
        };

        set({
          defaultTargetAgents: nextDefaults,
        });

        saveDefaultTargetAgents(nextDefaults).catch((error) => {
          console.error('保存默认 agents 失败，回滚状态:', error);
          set({
            defaultTargetAgents,
          });
        });
      },

      toggleDefaultTargetAgent: (scope, agentId) => {
        const current = get().defaultTargetAgents[scope];
        const nextAgents = current.includes(agentId)
          ? current.filter((id) => id !== agentId)
          : [...current, agentId];
        get().setDefaultTargetAgents(scope, nextAgents);
      },

      isDefaultTargetAgentSelected: (scope, agentId) => {
        return get().defaultTargetAgents[scope].includes(agentId);
      },
    }),
    {
      name: 'skill-deck-settings',
      // 只持久化 theme 和 locale；默认安装目标由后端 lock 文件持久化。
      partialize: (state) => ({
        theme: state.theme,
        locale: state.locale,
      }),
      onRehydrateStorage: () => (state) => {
        if (state) {
          applyTheme(state.theme);
          i18n.changeLanguage(state.locale);
        }
      },
    }
  )
);
