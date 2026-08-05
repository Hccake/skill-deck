import type {
  AgentId,
  InstallResponse,
} from '@/bindings';

export interface AdapterTargetSelection {
  agentId: AgentId;
  targetId: string;
}

export function hasFailedMutationUnits(response: InstallResponse): boolean {
  return response.units.some((unit) => unit.status !== 'succeeded');
}
