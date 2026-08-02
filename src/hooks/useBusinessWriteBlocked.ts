import {
  selectInstallWizardSessionBlocksWrites,
  useInstallWizardSessionStore,
} from '@/stores/install-wizard-session';
import { useMutationStore } from '@/stores/mutation';

export type BusinessWriteBlockReason =
  | 'mutationActive'
  | 'installWizardActive'
  | 'installWizardSyncing'
  | 'installWizardUnavailable';

export class BusinessWriteBlockedError extends Error {
  readonly reason: BusinessWriteBlockReason;

  constructor(reason: BusinessWriteBlockReason) {
    super(`Business writes are currently blocked: ${reason}`);
    this.name = 'BusinessWriteBlockedError';
    this.reason = reason;
  }
}

function installWizardBlockReason(): BusinessWriteBlockReason | null {
  const state = useInstallWizardSessionStore.getState();
  if (state.syncError) return 'installWizardUnavailable';
  if (state.loading) return 'installWizardSyncing';
  if (state.active) return 'installWizardActive';
  return null;
}

export function businessWriteBlockReason(): BusinessWriteBlockReason | null {
  if (useMutationStore.getState().activeMutation !== null) return 'mutationActive';
  return installWizardBlockReason();
}

export function assertBusinessWriteAvailable(): void {
  const reason = businessWriteBlockReason();
  if (reason) throw new BusinessWriteBlockedError(reason);
}

export function useBusinessWriteBlocked(): boolean {
  const mutationActive = useMutationStore((state) => state.activeMutation !== null);
  const installWizardBlocksWrites = useInstallWizardSessionStore(
    selectInstallWizardSessionBlocksWrites,
  );
  return mutationActive || installWizardBlocksWrites;
}

export function isBusinessWriteBlocked(): boolean {
  return businessWriteBlockReason() !== null;
}
