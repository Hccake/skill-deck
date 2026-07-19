import {
  deleteCustomAgent,
  deleteInvalidCustomAgent,
  saveCustomAgent,
} from '@/hooks/useTauriApi';
import { useAgentRegistryStore } from '@/stores/agent-registry';
import { useSettingsStore } from '@/stores/settings';
import { useSkillsDataStore } from '@/stores/skills-data';
import type {
  AgentDeleteResult,
  AgentId,
  AgentSettingsSnapshot,
  ContextRef,
  CustomAgentDefinition,
} from '@/bindings';

export function applyAgentRegistryMutationResult(settings: AgentSettingsSnapshot): void {
  useAgentRegistryStore.getState().invalidateAll();
  useSettingsStore.getState().invalidateAgentDefaults();
  useSkillsDataStore.getState().invalidateAgentProjections();
  useAgentRegistryStore.getState().acceptSettings(settings);
}

export interface AgentDefinitionWorkflow {
  save(
    context: ContextRef,
    draft: CustomAgentDefinition,
    expectedRevision: string,
  ): Promise<AgentSettingsSnapshot>;
  delete(
    context: ContextRef,
    id: AgentId,
    expectedRevision: string,
  ): Promise<AgentDeleteResult>;
  deleteInvalid(
    context: ContextRef,
    index: number,
    expectedRevision: string,
  ): Promise<AgentDeleteResult>;
}

export const agentDefinitionWorkflow: AgentDefinitionWorkflow = {
  async save(context, draft, expectedRevision) {
    const settings = await saveCustomAgent(context, draft, expectedRevision);
    applyAgentRegistryMutationResult(settings);
    return settings;
  },

  async delete(context, id, expectedRevision) {
    const result = await deleteCustomAgent(context, id, expectedRevision);
    applyAgentRegistryMutationResult(result.settings);
    return result;
  },

  async deleteInvalid(context, index, expectedRevision) {
    const result = await deleteInvalidCustomAgent(context, index, expectedRevision);
    applyAgentRegistryMutationResult(result.settings);
    return result;
  },
};
