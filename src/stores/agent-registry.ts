import { create } from 'zustand';
import {
  duplicateCustomAgentDraft,
  getAgentSettingsSnapshot,
  listAgents,
  previewCustomAgentDelete,
  validateCustomAgentDraft,
} from '@/hooks/useTauriApi';
import { contextKey, environmentKey } from '@/lib/context';
import { toAppError } from '@/utils/to-app-error';
import type {
  AgentDeleteImpact,
  AgentId,
  AgentRuntimeSnapshot,
  AgentSettingsSnapshot,
  AppError,
  ContextRef,
  CustomAgentDefinition,
  CustomAgentDraftValidation,
} from '@/bindings';

export interface AsyncSnapshot<T> {
  data: T | null;
  state: 'idle' | 'loading' | 'ready' | 'error' | 'stale';
  requestId: number;
  error: AppError | null;
}

interface AgentRegistryApi {
  getAgentSettingsSnapshot: typeof getAgentSettingsSnapshot;
  listAgents: typeof listAgents;
  validateCustomAgentDraft: typeof validateCustomAgentDraft;
  duplicateCustomAgentDraft: typeof duplicateCustomAgentDraft;
  previewCustomAgentDelete: typeof previewCustomAgentDelete;
}

interface AgentRegistryState {
  settingsByEnvironment: Record<string, AsyncSnapshot<AgentSettingsSnapshot>>;
  runtimeByContext: Record<string, AsyncSnapshot<AgentRuntimeSnapshot>>;
  loadSettings: (context: ContextRef) => Promise<void>;
  loadRuntime: (context: ContextRef) => Promise<void>;
  validateDraft: (
    context: ContextRef,
    draft: CustomAgentDefinition,
    lane?: 'background' | 'submit',
  ) => Promise<CustomAgentDraftValidation | null>;
  duplicateDraft: (sourceId: AgentId, newId: AgentId) => Promise<CustomAgentDefinition>;
  loadDeleteImpact: (
    context: ContextRef,
    id: AgentId,
    revision: string,
  ) => Promise<AgentDeleteImpact | null>;
  acceptSettings: (snapshot: AgentSettingsSnapshot) => void;
  invalidateRuntime: () => void;
  invalidateAll: () => void;
}

const api: AgentRegistryApi = {
  getAgentSettingsSnapshot,
  listAgents,
  validateCustomAgentDraft,
  duplicateCustomAgentDraft,
  previewCustomAgentDelete,
};

function emptySnapshot<T>(): AsyncSnapshot<T> {
  return { data: null, state: 'idle', requestId: 0, error: null };
}

function deleteImpactContextAgentKey(context: ContextRef, id: AgentId): string {
  return `${contextKey(context)}/${encodeURIComponent(id)}`;
}

export function createAgentRegistryStore(overrides: Partial<AgentRegistryApi> = {}) {
  const client = { ...api, ...overrides };
  const generations = new Map<string, number>();
  const nextGeneration = (key: string) => {
    const requestId = (generations.get(key) ?? 0) + 1;
    generations.set(key, requestId);
    return requestId;
  };
  const isCurrent = (key: string, requestId: number) => generations.get(key) === requestId;
  const invalidateGenerationPrefix = (prefix: string) => {
    for (const key of Array.from(generations.keys())) {
      if (key.startsWith(prefix)) nextGeneration(key);
    }
  };

  return create<AgentRegistryState>()((set) => {
    const invalidateKeys = (prefix: string, keys: Iterable<string>) => {
      for (const key of keys) nextGeneration(`${prefix}:${key}`);
    };

    return {
      settingsByEnvironment: {},
      runtimeByContext: {},

      loadSettings: async (context) => {
        const key = environmentKey(context.environment);
        const generationKey = `settings:${key}`;
        const requestId = nextGeneration(generationKey);
        set((state) => ({
          settingsByEnvironment: {
            ...state.settingsByEnvironment,
            [key]: {
              ...(state.settingsByEnvironment[key] ?? emptySnapshot()),
              state: 'loading',
              requestId,
              error: null,
            },
          },
        }));
        try {
          const data = await client.getAgentSettingsSnapshot(context);
          if (!isCurrent(generationKey, requestId)) return;
          set((state) => ({
            settingsByEnvironment: {
              ...state.settingsByEnvironment,
              [key]: { data, state: 'ready', requestId, error: null },
            },
          }));
        } catch (error) {
          if (!isCurrent(generationKey, requestId)) return;
          set((state) => ({
            settingsByEnvironment: {
              ...state.settingsByEnvironment,
              [key]: {
                ...(state.settingsByEnvironment[key] ?? emptySnapshot()),
                state: 'error',
                requestId,
                error: toAppError(error),
              },
            },
          }));
        }
      },

      loadRuntime: async (context) => {
        const key = contextKey(context);
        const generationKey = `runtime:${key}`;
        const requestId = nextGeneration(generationKey);
        set((state) => ({
          runtimeByContext: {
            ...state.runtimeByContext,
            [key]: {
              ...(state.runtimeByContext[key] ?? emptySnapshot()),
              state: 'loading',
              requestId,
              error: null,
            },
          },
        }));
        try {
          const data = await client.listAgents(context);
          if (!isCurrent(generationKey, requestId)) return;
          set((state) => ({
            runtimeByContext: {
              ...state.runtimeByContext,
              [key]: { data, state: 'ready', requestId, error: null },
            },
          }));
        } catch (error) {
          if (!isCurrent(generationKey, requestId)) return;
          set((state) => ({
            runtimeByContext: {
              ...state.runtimeByContext,
              [key]: {
                ...(state.runtimeByContext[key] ?? emptySnapshot()),
                state: 'error',
                requestId,
                error: toAppError(error),
              },
            },
          }));
        }
      },

      validateDraft: async (context, draft, lane = 'background') => {
        const key = contextKey(context);
        const generationKey = `validation:${lane}:${key}`;
        const requestId = nextGeneration(generationKey);
        try {
          const data = await client.validateCustomAgentDraft(context, draft);
          if (!isCurrent(generationKey, requestId)) return null;
          return data;
        } catch (error) {
          if (!isCurrent(generationKey, requestId)) return null;
          throw error;
        }
      },

      duplicateDraft: async (sourceId, newId) => client.duplicateCustomAgentDraft(sourceId, newId),

      loadDeleteImpact: async (context, id, revision) => {
        const generationKey = `delete-impact:${deleteImpactContextAgentKey(context, id)}`;
        const requestId = nextGeneration(generationKey);
        try {
          const data = await client.previewCustomAgentDelete(context, id, revision);
          if (!isCurrent(generationKey, requestId)) return null;
          return data;
        } catch (error) {
          if (!isCurrent(generationKey, requestId)) return null;
          throw error;
        }
      },

      acceptSettings: (snapshot) => {
        const key = environmentKey(snapshot.currentEnvironment);
        const requestId = nextGeneration(`settings:${key}`);
        set((state) => ({
          settingsByEnvironment: {
            ...state.settingsByEnvironment,
            [key]: { data: snapshot, state: 'ready', requestId, error: null },
          },
        }));
      },

      invalidateRuntime: () => set((state) => {
        invalidateKeys('runtime', Object.keys(state.runtimeByContext));
        return { runtimeByContext: {} };
      }),

      invalidateAll: () => set((state) => {
        invalidateKeys('settings', Object.keys(state.settingsByEnvironment));
        invalidateKeys('runtime', Object.keys(state.runtimeByContext));
        invalidateGenerationPrefix('validation:');
        invalidateGenerationPrefix('delete-impact:');
        return {
          settingsByEnvironment: {},
          runtimeByContext: {},
        };
      }),
    };
  });
}

export const useAgentRegistryStore = createAgentRegistryStore();
