import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import i18n from '@/i18n';
import {
  getDefaultTargetAgents,
  listAgents,
  saveDefaultTargetAgents,
} from '@/hooks/useTauriApi';
import { isMutationWriteBlocked } from './mutation';
import type { AgentInfo, DefaultTargetAgents } from '@/hooks/useTauriApi';
import type { AppError, EnvironmentRef } from '@/bindings';
import { environmentKey, globalContext } from '@/lib/context';
import {
  EMPTY_DEFAULT_TARGET_AGENTS,
  filterAdditionalAgentIds,
  migrateDefaultTargetAgents,
} from '@/lib/agentTargets';

export type Theme = 'light' | 'dark';
export type Locale = 'en' | 'zh-CN';

// CLI 默认选中的 agents（与 vercel-skills CLI 一致）
const DEFAULT_AGENTS: string[] = ['claude-code', 'opencode', 'codex'];

export interface AgentDefaultsSnapshot {
  agents: AgentInfo[];
  defaults: DefaultTargetAgents;
  loadState: 'idle' | 'loading' | 'ready' | 'error';
  loadRequestId: number;
  saveRequestId: number;
  saving: boolean;
  error: AppError | null;
}

interface SettingsState {
  // 主题和语言（保持 localStorage 持久化）
  theme: Theme;
  locale: Locale;
  setTheme: (theme: Theme) => void;
  setLocale: (locale: Locale) => void;
  toggleTheme: () => void;

  agentDefaultsByEnvironment: Record<string, AgentDefaultsSnapshot>;
  loadAgentDefaults: (environment: EnvironmentRef) => Promise<void>;
  saveAgentDefaults: (
    environment: EnvironmentRef,
    defaults: DefaultTargetAgents,
  ) => Promise<void>;
}

const applyTheme = (theme: Theme) => {
  if (typeof window === 'undefined') return;

  const root = document.documentElement;
  root.classList.remove('light', 'dark');
  root.classList.add(theme);
};

function emptyAgentDefaultsSnapshot(): AgentDefaultsSnapshot {
  return {
    agents: [],
    defaults: {
      global: [...EMPTY_DEFAULT_TARGET_AGENTS.global],
      project: [...EMPTY_DEFAULT_TARGET_AGENTS.project],
    },
    loadState: 'idle',
    loadRequestId: 0,
    saveRequestId: 0,
    saving: false,
    error: null,
  };
}

function toAppError(error: unknown): AppError {
  if (error && typeof error === 'object' && 'kind' in error) {
    return error as AppError;
  }
  return {
    kind: 'custom',
    data: {
      message: error instanceof Error ? error.message : String(error),
    },
  };
}

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

      agentDefaultsByEnvironment: {},

      loadAgentDefaults: async (environment) => {
        const key = environmentKey(environment);
        const context = globalContext(environment);
        let requestId = 0;
        set((state) => {
          const current = state.agentDefaultsByEnvironment[key]
            ?? emptyAgentDefaultsSnapshot();
          requestId = current.loadRequestId + 1;
          return {
            agentDefaultsByEnvironment: {
              ...state.agentDefaultsByEnvironment,
              [key]: {
                ...current,
                loadState: 'loading',
                loadRequestId: requestId,
                error: null,
              },
            },
          };
        });
        try {
          const agentsPromise = listAgents(context);
          const targetDefaultsPromise = getDefaultTargetAgents(context).catch(() => null);
          const [agents, targetDefaults] = await Promise.all([
            agentsPromise,
            targetDefaultsPromise,
          ]);
          const migratedDefaults = migrateDefaultTargetAgents(DEFAULT_AGENTS, agents);
          const defaults = targetDefaults
            ? {
                global: filterAdditionalAgentIds(targetDefaults.global, agents, 'global'),
                project: filterAdditionalAgentIds(targetDefaults.project, agents, 'project'),
              }
            : migratedDefaults;
          set((state) => {
            const current = state.agentDefaultsByEnvironment[key];
            if (!current || current.loadRequestId !== requestId) return state;
            return {
              agentDefaultsByEnvironment: {
                ...state.agentDefaultsByEnvironment,
                [key]: {
                  ...current,
                  agents,
                  defaults,
                  loadState: 'ready',
                  error: null,
                },
              },
            };
          });
        } catch (error) {
          set((state) => {
            const current = state.agentDefaultsByEnvironment[key];
            if (!current || current.loadRequestId !== requestId) return state;
            return {
              agentDefaultsByEnvironment: {
                ...state.agentDefaultsByEnvironment,
                [key]: {
                  ...current,
                  loadState: 'error',
                  error: toAppError(error),
                },
              },
            };
          });
        }
      },

      saveAgentDefaults: async (environment, defaults) => {
        if (isMutationWriteBlocked()) return;
        const key = environmentKey(environment);
        const context = globalContext(environment);
        const current = get().agentDefaultsByEnvironment[key]
          ?? emptyAgentDefaultsSnapshot();
        const previousDefaults = current.defaults;
        const nextDefaults = {
          global: filterAdditionalAgentIds(defaults.global, current.agents, 'global'),
          project: filterAdditionalAgentIds(defaults.project, current.agents, 'project'),
        };
        const requestId = current.saveRequestId + 1;
        set((state) => ({
          agentDefaultsByEnvironment: {
            ...state.agentDefaultsByEnvironment,
            [key]: {
              ...(state.agentDefaultsByEnvironment[key] ?? current),
              defaults: nextDefaults,
              saveRequestId: requestId,
              saving: true,
              error: null,
            },
          },
        }));
        try {
          await saveDefaultTargetAgents(context, nextDefaults);
        } catch (error) {
          set((state) => {
            const latest = state.agentDefaultsByEnvironment[key];
            if (!latest || latest.saveRequestId !== requestId) return state;
            return {
              agentDefaultsByEnvironment: {
                ...state.agentDefaultsByEnvironment,
                [key]: {
                  ...latest,
                  defaults: previousDefaults,
                  error: toAppError(error),
                },
              },
            };
          });
        } finally {
          set((state) => {
            const latest = state.agentDefaultsByEnvironment[key];
            if (!latest || latest.saveRequestId !== requestId) return state;
            return {
              agentDefaultsByEnvironment: {
                ...state.agentDefaultsByEnvironment,
                [key]: { ...latest, saving: false },
              },
            };
          });
        }
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
