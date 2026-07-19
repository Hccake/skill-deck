import { commands } from '@/bindings';
import type { LifecycleAction, LifecycleActionOutcome } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

export async function executeLifecycleAction(
  action: LifecycleAction,
): Promise<LifecycleActionOutcome> {
  const result = await commands.executeLifecycleAction(action);
  if (result.status === 'error') throw result.error;

  const outcome = result.data;
  if (outcome.status === 'blocked') {
    useMutationStore.getState().acceptSnapshot({
      revision: outcome.snapshot.revision,
      active: outcome.snapshot.mutation,
    });
  }
  return outcome;
}
