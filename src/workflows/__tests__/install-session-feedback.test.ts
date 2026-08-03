import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import {
  continueInstallFlow,
  runBusinessWrite,
} from '../install-session-feedback';

const mocks = vi.hoisted(() => ({
  focusInstallWizard: vi.fn(),
  toastWarning: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  focusInstallWizard: (...args: unknown[]) => mocks.focusInstallWizard(...args),
}));

vi.mock('sonner', () => ({
  toast: {
    warning: mocks.toastWarning,
    error: mocks.toastError,
  },
}));

vi.mock('@/i18n', () => ({
  default: { t: (key: string) => key },
}));

describe('install session feedback', () => {
  const refreshSession = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    refreshSession.mockResolvedValue(undefined);
    mocks.focusInstallWizard.mockResolvedValue(true);
    useInstallWizardSessionStore.setState({ refreshSession });
  });

  it('returns the completed business write value without feedback', async () => {
    await expect(runBusinessWrite(async () => 'saved')).resolves.toEqual({
      status: 'completed',
      value: 'saved',
    });
    expect(mocks.toastWarning).not.toHaveBeenCalled();
  });

  it('consumes an install-flow race and refreshes the shared session state', async () => {
    await expect(runBusinessWrite(async () => {
      throw { kind: 'installWizardActive' };
    })).resolves.toEqual({
      status: 'notRun',
      reason: 'installFlowActive',
    });

    expect(mocks.toastWarning).toHaveBeenCalledWith(
      'installWizardSession.operationNotRun',
      expect.objectContaining({
        id: 'install-session-operation-not-run',
        action: expect.objectContaining({
          label: 'installWizardSession.continueOperation',
        }),
      }),
    );
    expect(refreshSession).toHaveBeenCalledTimes(1);
  });

  it('consumes an Agent application error that wraps the install-flow race', async () => {
    await expect(runBusinessWrite(async () => {
      throw { kind: 'application', error: { kind: 'installWizardActive' } };
    })).resolves.toEqual({
      status: 'notRun',
      reason: 'installFlowActive',
    });
  });

  it('uses the same toast id for repeated competition feedback', async () => {
    const denied = () => runBusinessWrite(async () => {
      throw { kind: 'installWizardActive' };
    });

    await denied();
    await denied();

    expect(mocks.toastWarning).toHaveBeenCalledTimes(2);
    expect(mocks.toastWarning.mock.calls.map((call) => call[1].id))
      .toEqual(['install-session-operation-not-run', 'install-session-operation-not-run']);
  });

  it('keeps the notRun outcome when refreshing the shared session fails', async () => {
    refreshSession.mockRejectedValue(new Error('refresh failed'));

    await expect(runBusinessWrite(async () => {
      throw { kind: 'installWizardActive' };
    })).resolves.toEqual({
      status: 'notRun',
      reason: 'installFlowActive',
    });
  });

  it('continues the installation flow from the competition toast', async () => {
    await runBusinessWrite(async () => {
      throw { kind: 'installWizardActive' };
    });
    const options = mocks.toastWarning.mock.calls[0][1];

    options.action.onClick();

    await vi.waitFor(() => expect(mocks.focusInstallWizard).toHaveBeenCalledTimes(1));
  });

  it('refreshes stale state when the installation window no longer exists', async () => {
    mocks.focusInstallWizard.mockResolvedValue(false);

    await continueInstallFlow();

    expect(refreshSession).toHaveBeenCalledTimes(1);
  });

  it('does not report a focus failure when the missing-window refresh fails', async () => {
    mocks.focusInstallWizard.mockResolvedValue(false);
    refreshSession.mockRejectedValue(new Error('refresh failed'));

    await expect(continueInstallFlow()).resolves.toBeUndefined();

    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it('leaves unrelated failures for the original business workflow', async () => {
    const error = new Error('save failed');

    await expect(runBusinessWrite(async () => {
      throw error;
    })).rejects.toBe(error);
    expect(mocks.toastWarning).not.toHaveBeenCalled();
    expect(refreshSession).not.toHaveBeenCalled();
  });
});
