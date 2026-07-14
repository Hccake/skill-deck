/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import App from '../App';

const mocks = vi.hoisted(() => ({
  discoverEnvironments: vi.fn(),
  refreshWorkspace: vi.fn(),
  listen: vi.fn().mockResolvedValue(() => undefined),
  requestClose: vi.fn().mockResolvedValue('performed'),
  monitorEnvironmentRuntime: vi.fn(),
  wizardResultHandler: null as null | (() => void),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (event: string, handler: () => void) => {
    if (event === 'wizard-result') mocks.wizardResultHandler = handler;
    return mocks.listen(event, handler);
  },
}));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@/components/layout/Header', () => ({ Header: () => null }));
vi.mock('@/components/layout/MutationStatusBar', () => ({
  MutationStatusBar: () => <div>mutation-status-bar</div>,
}));
vi.mock('@/components/layout/MutationInterruptionDialog', () => ({
  MutationInterruptionDialog: () => <div>close-protection-dialog</div>,
}));
vi.mock('@/hooks/useProtectedWindowClose', () => ({
  useProtectedWindowClose: () => ({
    requestClose: mocks.requestClose,
    dialogProps: {
      open: false,
      action: 'close',
      cancelable: false,
      cancelling: false,
      onContinueWaiting: vi.fn(),
      onCancelAndContinue: vi.fn(),
    },
  }),
}));
vi.mock('@/hooks/useEnvironmentRuntimeMonitor', () => ({
  useEnvironmentRuntimeMonitor: mocks.monitorEnvironmentRuntime,
}));
vi.mock('@/pages/SkillsPage', () => ({ SkillsPage: () => null }));
vi.mock('@/pages/DiscoverPage', () => ({ DiscoverPage: () => null }));
vi.mock('@/pages/SettingsPage', () => ({ SettingsPage: () => null }));
vi.mock('@/pages/WizardPage', () => ({ WizardPage: () => null }));
vi.mock('@/components/ui/sonner', () => ({ Toaster: () => null }));
vi.mock('@/components/ui/tooltip', () => ({ TooltipProvider: ({ children }: { children: React.ReactNode }) => children }));
vi.mock('@/components/update-dialog', () => ({ UpdateDialog: () => null }));
vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: { refreshWorkspace: typeof mocks.refreshWorkspace }) => unknown) =>
    selector({ refreshWorkspace: mocks.refreshWorkspace }),
}));
vi.mock('@/stores/updater', () => ({
  useUpdaterStore: Object.assign(
    () => ({ status: 'idle', checkForUpdate: vi.fn(), shouldAutoCheck: () => false }),
    { getState: () => ({ error: null }) },
  ),
}));
vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: { discover: () => Promise<void> }) => unknown) => selector({
    discover: mocks.discoverEnvironments,
  }),
}));
vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: {
    getState: () => ({
      selectedContext: {
        environment: { kind: 'host' },
        scope: { scope: 'global' },
      },
    }),
  },
}));

describe('App', () => {
  it('discovers environments whenever the main application starts', async () => {
    mocks.discoverEnvironments.mockResolvedValue(undefined);

    render(<App />);

    await waitFor(() => expect(mocks.discoverEnvironments).toHaveBeenCalledTimes(1));
  });

  it('refreshes the committed Host Global workspace after the wizard completes', async () => {
    mocks.refreshWorkspace.mockResolvedValue(undefined);

    render(<App />);
    await waitFor(() => expect(mocks.wizardResultHandler).not.toBeNull());
    act(() => mocks.wizardResultHandler?.());

    await waitFor(() => expect(mocks.refreshWorkspace).toHaveBeenCalledWith({
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    }));
  });

  it('mounts the global mutation status in the main window', () => {
    render(<App />);

    expect(screen.getByText('mutation-status-bar')).toBeDefined();
  });

  it('mounts close protection in the main window', () => {
    render(<App />);

    expect(screen.getByText('close-protection-dialog')).toBeDefined();
  });

  it('mounts the environment runtime monitor once in the main layout', () => {
    const callsBeforeRender = mocks.monitorEnvironmentRuntime.mock.calls.length;

    render(<App />);

    expect(mocks.monitorEnvironmentRuntime).toHaveBeenCalledTimes(callsBeforeRender + 1);
  });
});
