import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import i18n from '@/i18n';
import {
  getGithubCredentialStatus,
  clearGithubCredential as clearGithubCredentialCommand,
  saveGithubCredential as saveGithubCredentialCommand,
} from '@/hooks/useTauriApi';
import { useSkillsDataStore } from './skills-data';
import { isBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { runBusinessWrite } from '@/workflows/install-session-feedback';
import type {
  AppError,
  GithubCredentialClearResult,
  GithubCredentialSaveResult,
  GithubCredentialStatus,
} from '@/bindings';

export type Theme = 'light' | 'dark';
export type Locale = 'en' | 'zh-CN';

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
        if (isBusinessWriteBlocked()) return null;
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
          const outcome = await runBusinessWrite(() => saveGithubCredentialCommand(token));
          if (outcome.status === 'notRun') return null;
          const result = outcome.value;
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: {
              ...state.githubCredential,
              status: result.saved || result.status.storage === 'unavailable'
                ? result.status
                : state.githubCredential.status,
              loadState: 'ready',
              error: null,
            },
          } : state);
          if (result.saved && !result.warnings.includes('suppressionCleanupFailed')) {
            useSkillsDataStore.getState().clearNativeGithubProviderCooldown();
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
        if (isBusinessWriteBlocked()) return null;
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
          const outcome = await runBusinessWrite(() => clearGithubCredentialCommand());
          if (outcome.status === 'notRun') return null;
          const result = outcome.value;
          set((state) => state.githubCredential.requestId === requestId ? {
            githubCredential: {
              ...state.githubCredential,
              status: result.cleared ? result.status : state.githubCredential.status,
              loadState: 'ready',
              error: null,
            },
          } : state);
          if (result.cleared && !result.warnings.includes('suppressionCleanupFailed')) {
            useSkillsDataStore.getState().clearNativeGithubProviderCooldown();
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
      // 只持久化 theme 和 locale。
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
