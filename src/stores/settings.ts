import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import i18n from '@/i18n';
import { getLastSelectedAgents, saveLastSelectedAgents, listAgents } from '@/hooks/useTauriApi';

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
  defaultAgents: string[];
  agentsLoaded: boolean;
  loadDefaultAgents: () => Promise<void>;
  setDefaultAgents: (agents: string[]) => void;
  toggleAgent: (agentId: string) => void;
  isAgentSelected: (agentId: string) => boolean;
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
      defaultAgents: [],
      agentsLoaded: false,

      loadDefaultAgents: async () => {
        try {
          // 并行获取 lastSelectedAgents 和 agents 列表
          const [lastSelected, agents] = await Promise.all([
            getLastSelectedAgents(),
            listAgents(),
          ]);

          const detectedIds = new Set<string>(
            agents.filter((a) => a.detected).map((a) => a.id)
          );

          let defaultAgents = lastSelected;

          // 如果没有历史记录，使用 CLI 默认值（过滤为已检测到的）
          if (defaultAgents.length === 0) {
            defaultAgents = DEFAULT_AGENTS.filter((id) => detectedIds.has(id));

            // 后台保存默认配置，不阻塞 UI
            if (defaultAgents.length > 0) {
              saveLastSelectedAgents(defaultAgents).catch((err) =>
                console.error('保存默认 agents 失败:', err)
              );
            }
          }

          set({ defaultAgents, agentsLoaded: true });
        } catch (error) {
          console.error('加载默认 agents 失败:', error);

          // 降级处理：使用 CLI 默认值
          try {
            const agents = await listAgents();
            const detectedIds = new Set<string>(
              agents.filter((a) => a.detected).map((a) => a.id)
            );
            const defaultAgents = DEFAULT_AGENTS.filter((id) =>
              detectedIds.has(id)
            );
            set({ defaultAgents, agentsLoaded: true });
          } catch {
            // 完全失败，仅标记加载完成
            set({ agentsLoaded: true });
          }
        }
      },

      setDefaultAgents: (agents: string[]) => {
        set({ defaultAgents: agents });

        // 异步保存
        saveLastSelectedAgents(agents).catch((error) => {
          console.error('保存默认 agents 失败:', error);
        });
      },

      toggleAgent: (agentId: string) => {
        const { defaultAgents } = get();
        const isSelected = defaultAgents.includes(agentId);

        // 乐观更新
        const newDefaultAgents = isSelected
          ? defaultAgents.filter((id) => id !== agentId)
          : [...defaultAgents, agentId];

        set({ defaultAgents: newDefaultAgents });

        // 异步保存，失败时回滚
        saveLastSelectedAgents(newDefaultAgents).catch((error) => {
          console.error('保存默认 agents 失败，回滚状态:', error);
          set({ defaultAgents }); // 回滚
        });
      },

      isAgentSelected: (agentId: string) => {
        return get().defaultAgents.includes(agentId);
      },
    }),
    {
      name: 'skill-deck-settings',
      // 只持久化 theme 和 locale，不持久化 defaultAgents（后端持久化）
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
