import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import i18n from '@/i18n';
import {
  getDefaultTargetAgents,
  getGithubCredentialStatus,
  listAgentSelectionGroups,
  clearGithubCredential,
  saveGithubCredential,
  saveDefaultTargetAgents,
} from '@/hooks/useTauriApi';
import { useAgentRegistryStore } from './agent-registry';
import { useSkillsDataStore } from './skills-data';
import { isMutationWriteBlocked } from './mutation';
import type { DefaultTargetAgents } from '@/hooks/useTauriApi';
import type {
  AgentSelectionGroups,
  AppError,
  EnvironmentRef,
  GithubCredentialClearResult,
  GithubCredentialSaveResult,
  GithubCredentialStatus,
  ResolvedAgent,
} from '@/bindings';
import { contextKey, environmentKey, globalContext } from '@/lib/context';
import { agentId, agentsForScope } from '@/lib/agents';
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
  agents: ResolvedAgent[];
  selectionGroups: AgentSelectionGroups;
  registryRevision: string;
  defaults: DefaultTargetAgents;
  loadState: 'idle' | 'loading' | 'ready' | 'error' | 'stale';
  loadRequestId: number;
  saveRequestId: number;
  saving: boolean;
  error: AppError | null;
}

export interface GithubCredentialSnapshot {
  status: GithubCredentialStatus | null;
  loadState: 'idle' | 'loading' | 'ready' | 'error';
  requestId: number;
  saving: boolean;
  clearing: boolean;
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
  invalidateAgentDefaults: () => void;
  loadAgentDefaults: (environment: EnvironmentRef) => Promise<void>;
  saveAgentDefaults: (
    environment: EnvironmentRef,
    defaults: DefaultTargetAgents,
  ) => Promise<void>;

  githubCredential: GithubCredentialSnapshot;
  loadGithubCredential: () => Promise<void>;
  saveGithubCredential: (token: string) => Promise<GithubCredentialSaveResult | null>;
  clearGithubCredential: () => Promise<GithubCredentialClearResult | null>;
}

export const applyPersistedAppearance = (theme: Theme) => {
  if (typeof window === 'undefined') return;

  const root = document.documentElement;
  root.classList.remove('light', 'dark');
  root.classList.add(theme);
};

function emptyAgentDefaultsSnapshot(): AgentDefaultsSnapshot {
  return {
    agents: [],
    selectionGroups: { global: [], project: [] },
    registryRevision: '',
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

function emptyGithubCredentialSnapshot(): GithubCredentialSnapshot {
  return {
    status: null,
    loadState: 'idle',
    requestId: 0,
    saving: false,
    clearing: false,
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
        applyPersistedAppearance(theme);
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

      invalidateAgentDefaults: () => set({ agentDefaultsByEnvironment: {} }),

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
          const runtimePromise = useAgentRegistryStore.getState().loadRuntime(context);
          const selectionGroupsPromise = listAgentSelectionGroups(context);
          const targetDefaultsPromise = getDefaultTargetAgents(context).catch(() => null);
          const [, selectionGroups, targetDefaults] = await Promise.all([
            runtimePromise,
            selectionGroupsPromise,
            targetDefaultsPromise,
          ]);
          const runtimeState = useAgentRegistryStore.getState()
            .runtimeByContext[contextKey(context)];
          const runtimeSnapshot = runtimeState?.data;
          if (!runtimeSnapshot) {
            if (runtimeState?.error) throw runtimeState.error;
            throw new Error('Agent runtime snapshot is unavailable');
          }
          const globalAgents = agentsForScope(runtimeSnapshot, 'global');
          const globalAgentIds = new Set(globalAgents.map(agentId));
          const agents = [
            ...globalAgents,
            ...agentsForScope(runtimeSnapshot, 'project')
              .filter((agent) => !globalAgentIds.has(agentId(agent))),
          ];
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
                  selectionGroups,
                  registryRevision: runtimeSnapshot.registryRevision,
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
          await saveDefaultTargetAgents(
            context,
            nextDefaults,
            current.registryRevision,
          );
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
                  loadState: 'stale',
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

      githubCredential: emptyGithubCredentialSnapshot(),

      loadGithubCredential: async () => {
        const requestId = get().githubCredential.requestId + 1;
        set((state) => ({
          githubCredential: {
            ...state.githubCredential,
            loadState: 'loading',
            requestId,
            error: null,
          },
        }));
        try {
          const status = await getGithubCredentialStatus();
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: {
              ...state.githubCredential,
              status,
              loadState: 'ready',
              error: null,
            },
          } : state);
        } catch (error) {
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: {
              ...state.githubCredential,
              loadState: 'error',
              error: toAppError(error),
            },
          } : state);
        }
      },

      saveGithubCredential: async (token) => {
        const requestId = get().githubCredential.requestId + 1;
        set((state) => ({
          githubCredential: {
            ...state.githubCredential,
            requestId,
            saving: true,
            error: null,
          },
        }));
        try {
          const result = await saveGithubCredential(token);
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: {
              ...state.githubCredential,
              status: result.saved ? result.status : state.githubCredential.status,
              loadState: 'ready',
              error: null,
            },
          } : state);
          if (result.saved && !result.warnings.includes('suppressionCleanupFailed')) {
            useSkillsDataStore.getState().clearHostGithubProviderCooldown();
          }
          return result;
        } catch (error) {
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: {
              ...state.githubCredential,
              loadState: 'error',
              error: toAppError(error),
            },
          } : state);
          return null;
        } finally {
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: { ...state.githubCredential, saving: false },
          } : state);
        }
      },

      clearGithubCredential: async () => {
        const requestId = get().githubCredential.requestId + 1;
        set((state) => ({
          githubCredential: {
            ...state.githubCredential,
            requestId,
            clearing: true,
            error: null,
          },
        }));
        try {
          const result = await clearGithubCredential();
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: {
              ...state.githubCredential,
              status: result.cleared ? result.status : state.githubCredential.status,
              loadState: 'ready',
              error: null,
            },
          } : state);
          if (result.cleared && !result.warnings.includes('suppressionCleanupFailed')) {
            useSkillsDataStore.getState().clearHostGithubProviderCooldown();
          }
          return result;
        } catch (error) {
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: {
              ...state.githubCredential,
              loadState: 'error',
              error: toAppError(error),
            },
          } : state);
          return null;
        } finally {
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: { ...state.githubCredential, clearing: false },
          } : state);
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
          applyPersistedAppearance(state.theme);
          i18n.changeLanguage(state.locale);
        }
      },
    }
  )
);
