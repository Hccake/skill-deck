/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import App from '../App';

const mocks = vi.hoisted(() => ({
  refreshWorkspace: vi.fn(),
  listen: vi.fn().mockResolvedValue(() => undefined),
  monitorEnvironmentRuntime: vi.fn(),
  agentConfigurationRouter: vi.fn(),
  recoveryCenter: vi.fn(),
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
vi.mock('@/lifecycle/WindowLifecycleProvider', () => ({
  WindowLifecycleProvider: ({ children }: { children: React.ReactNode }) => (
    <div>
      <span>window-lifecycle-provider</span>
      {children}
    </div>
  ),
}));
vi.mock('@/lifecycle/UnsavedChangesProvider', () => ({
  UnsavedChangesProvider: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock('@/lifecycle/unsaved-changes-context', () => ({
  useOptionalUnsavedChanges: () => null,
}));
vi.mock('@/components/recovery/RecoveryCenter', () => ({
  RecoveryCenter: () => {
    mocks.recoveryCenter();
    return null;
  },
}));
vi.mock('@/components/settings/AgentConfigurationRequestRouter', () => ({
  AgentConfigurationRequestRouter: () => {
    mocks.agentConfigurationRouter();
    return null;
  },
}));
vi.mock('@/hooks/useEnvironmentRuntimeMonitor', () => ({
  useEnvironmentRuntimeMonitor: mocks.monitorEnvironmentRuntime,
}));
vi.mock('@/pages/SkillsPage', () => ({ SkillsPage: () => null }));
vi.mock('@/pages/DiscoverPage', () => ({ DiscoverPage: () => null }));
vi.mock('@/pages/SettingsPage', () => ({ SettingsPage: () => null }));
vi.mock('@/pages/WizardPage', () => ({ WizardPage: () => null }));
vi.mock('@/components/ui/sonner', () => ({ Toaster: () => <div>window-toaster</div> }));
vi.mock('@/components/ui/tooltip', () => ({ TooltipProvider: ({ children }: { children: React.ReactNode }) => children }));
vi.mock('@/components/update-dialog', () => ({ UpdateDialog: () => null }));
vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: { refreshWorkspace: typeof mocks.refreshWorkspace }) => unknown) =>
    selector({ refreshWorkspace: mocks.refreshWorkspace }),
}));
vi.mock('@/stores/updater', () => ({
  useUpdaterStore: Object.assign((selector: (state: unknown) => unknown) => selector({
    status: 'idle',
    dialogVisible: false,
    checkForUpdate: vi.fn(),
    shouldAutoCheck: () => false,
  }), { getState: () => ({ error: null }) }),
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
  beforeEach(() => {
    mocks.refreshWorkspace.mockClear();
    mocks.listen.mockClear();
    mocks.monitorEnvironmentRuntime.mockClear();
    mocks.agentConfigurationRouter.mockClear();
    mocks.recoveryCenter.mockClear();
    mocks.wizardResultHandler = null;
    window.history.replaceState({}, '', '/');
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

  it('mounts lifecycle error toasts in the wizard window', () => {
    window.history.pushState({}, '', '/wizard');
    window.dispatchEvent(new PopStateEvent('popstate'));

    render(<App />);

    expect(screen.getByText('window-toaster')).toBeDefined();
  });

  it('does not mount main-window Environment, recovery, or Agent routing owners in the wizard', () => {
    window.history.pushState({}, '', '/wizard');
    window.dispatchEvent(new PopStateEvent('popstate'));

    render(<App />);

    expect(mocks.monitorEnvironmentRuntime).not.toHaveBeenCalled();
    expect(mocks.recoveryCenter).not.toHaveBeenCalled();
    expect(mocks.agentConfigurationRouter).not.toHaveBeenCalled();
  });
});
