import { toast } from 'sonner';
import { focusInstallWizard } from '@/hooks/useTauriApi';
import i18n from '@/i18n';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

const OPERATION_NOT_RUN_TOAST_ID = 'install-session-operation-not-run';

export type BusinessWriteOutcome<T> =
  | { status: 'completed'; value: T }
  | { status: 'notRun'; reason: 'installFlowActive' };

export async function continueInstallFlow(): Promise<void> {
  const focused = await focusInstallWizard();
  if (!focused) {
    try {
      await useInstallWizardSessionStore.getState().refreshSession();
    } catch (error) {
      console.error('Failed to refresh installation session after missing window:', error);
    }
  }
}

function installFlowIsActive(error: unknown): boolean {
  if (error === null || typeof error !== 'object' || !('kind' in error)) return false;
  if (error.kind === 'installWizardActive') return true;
  return error.kind === 'application'
    && 'error' in error
    && error.error !== null
    && typeof error.error === 'object'
    && 'kind' in error.error
    && error.error.kind === 'installWizardActive';
}

function continueFromToast(): void {
  void continueInstallFlow().catch((error) => {
    console.error('Failed to continue installation flow:', error);
    toast.error(i18n.t('installWizardSession.focusFailed'));
  });
}

export async function runBusinessWrite<T>(
  operation: () => Promise<T>,
): Promise<BusinessWriteOutcome<T>> {
  try {
    return { status: 'completed', value: await operation() };
  } catch (error) {
    if (!installFlowIsActive(error)) throw error;

    toast.warning(i18n.t('installWizardSession.operationNotRun'), {
      id: OPERATION_NOT_RUN_TOAST_ID,
      action: {
        label: i18n.t('installWizardSession.continueOperation'),
        onClick: continueFromToast,
      },
    });
    try {
      await useInstallWizardSessionStore.getState().refreshSession();
    } catch (refreshError) {
      console.error('Failed to refresh installation session after denied write:', refreshError);
    }
    return { status: 'notRun', reason: 'installFlowActive' };
  }
}
