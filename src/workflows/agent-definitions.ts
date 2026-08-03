import {
  deleteCustomAgent,
  deleteInvalidCustomAgent,
  saveCustomAgent,
} from '@/hooks/useTauriApi';
import { useAgentRegistryStore } from '@/stores/agent-registry';
import { useSkillsDataStore } from '@/stores/skills-data';
import type {
  AgentDeleteResult,
  AgentId,
  AgentSettingsSnapshot,
  ContextRef,
  CustomAgentDefinition,
} from '@/bindings';
import { assertBusinessWriteAvailable } from '@/hooks/useBusinessWriteBlocked';
import { runBusinessWrite } from './install-session-feedback';

export function applyAgentRegistryMutationResult(settings: AgentSettingsSnapshot): void {
  useAgentRegistryStore.getState().invalidateAll();
  useSkillsDataStore.getState().invalidateAgentProjections();
  useAgentRegistryStore.getState().acceptSettings(settings);
}

export interface AgentDefinitionWorkflow {
  save(
    context: ContextRef,
    draft: CustomAgentDefinition,
    originalId: AgentId | null,
    expectedRevision: string,
  ): Promise<AgentSettingsSnapshot | null>;
  delete(
    context: ContextRef,
    id: AgentId,
    expectedRevision: string,
  ): Promise<AgentDeleteResult | null>;
  deleteInvalid(
    context: ContextRef,
    index: number,
    expectedRevision: string,
  ): Promise<AgentDeleteResult | null>;
}

export const agentDefinitionWorkflow: AgentDefinitionWorkflow = {
  async save(context, draft, originalId, expectedRevision) {
    assertBusinessWriteAvailable();
    const outcome = await runBusinessWrite(() => (
      saveCustomAgent(context, draft, originalId, expectedRevision)
    ));
    if (outcome.status === 'notRun') return null;
    const settings = outcome.value;
    applyAgentRegistryMutationResult(settings);
    return settings;
  },

  async delete(context, id, expectedRevision) {
    assertBusinessWriteAvailable();
    const outcome = await runBusinessWrite(() => deleteCustomAgent(context, id, expectedRevision));
    if (outcome.status === 'notRun') return null;
    const result = outcome.value;
    applyAgentRegistryMutationResult(result.settings);
    return result;
  },

  async deleteInvalid(context, index, expectedRevision) {
    assertBusinessWriteAvailable();
    const outcome = await runBusinessWrite(() => (
      deleteInvalidCustomAgent(context, index, expectedRevision)
    ));
    if (outcome.status === 'notRun') return null;
    const result = outcome.value;
    applyAgentRegistryMutationResult(result.settings);
    return result;
  },
};
