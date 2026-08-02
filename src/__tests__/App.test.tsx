/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import App from '../App';

const mocks = vi.hoisted(() => ({
  refreshWorkspace: vi.fn(),
  listen: vi.fn().mockResolvedValue(() => undefined),
  monitorEnvironmentRuntime: vi.fn(),
  monitorInstallWizardSession: vi.fn(),
  recoveryCenter: vi.fn(),
  checkForUpdate: vi.fn(),
  shouldAutoCheck: vi.fn(() => false),
  wizardResultHandler: null as null | ((event: {
    payload: { context: { environment: { kind: 'wsl'; distro_name: string }; scope: { scope: 'project'; project_id: string } }; mutatedSkillNames?: string[] };
  }) => void),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (event: string, handler: (event: Parameters<NonNullable<typeof mocks.wizardResultHandler>>[0]) => void) => {
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
vi.mock('@/hooks/useEnvironmentRuntimeMonitor', () => ({
  useEnvironmentRuntimeMonitor: mocks.monitorEnvironmentRuntime,
}));
vi.mock('@/hooks/useInstallWizardSessionMonitor', () => ({
  useInstallWizardSessionMonitor: mocks.monitorInstallWizardSession,
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
    checkForUpdate: mocks.checkForUpdate,
    shouldAutoCheck: mocks.shouldAutoCheck,
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
    mocks.monitorInstallWizardSession.mockClear();
    mocks.recoveryCenter.mockClear();
    mocks.checkForUpdate.mockClear();
    mocks.shouldAutoCheck.mockReset();
    mocks.shouldAutoCheck.mockReturnValue(false);
    mocks.wizardResultHandler = null;
    window.history.replaceState({}, '', '/');
  });

  it('refreshes the committed Host Global workspace after the wizard completes', async () => {
    mocks.refreshWorkspace.mockResolvedValue(undefined);

    render(<App />);
    await waitFor(
      () => expect(mocks.wizardResultHandler).not.toBeNull(),
      { timeout: 5000 },
    );
    act(() => mocks.wizardResultHandler?.({
      payload: {
        context: {
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          scope: { scope: 'project', project_id: 'project-a' },
        },
        mutatedSkillNames: ['toolkit'],
      },
    }));

    await waitFor(
      () => expect(mocks.refreshWorkspace).toHaveBeenCalledWith({
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        scope: { scope: 'project', project_id: 'project-a' },
      }, { origin: 'selfMutation', mutatedSkillNames: ['toolkit'] }),
      { timeout: 5000 },
    );
  });

  it('mounts the global mutation status in the main window', async () => {
    render(<App />);

    expect(await screen.findByText('mutation-status-bar', {}, { timeout: 5000 })).toBeDefined();
    expect(mocks.monitorInstallWizardSession).toHaveBeenCalledTimes(1);
  });

  it('mounts lifecycle error toasts in the wizard window', () => {
    window.history.pushState({}, '', '/wizard');
    window.dispatchEvent(new PopStateEvent('popstate'));

    render(<App />);

    expect(screen.getByText('window-toaster')).toBeDefined();
  });

  it('does not mount main-window Environment or recovery owners in the wizard', () => {
    window.history.pushState({}, '', '/wizard');
    window.dispatchEvent(new PopStateEvent('popstate'));

    render(<App />);

    expect(mocks.monitorEnvironmentRuntime).not.toHaveBeenCalled();
    expect(mocks.monitorInstallWizardSession).not.toHaveBeenCalled();
    expect(mocks.recoveryCenter).not.toHaveBeenCalled();
  });

  it('does not check for application updates from the install wizard', () => {
    mocks.shouldAutoCheck.mockReturnValue(true);
    window.history.pushState({}, '', '/wizard');
    window.dispatchEvent(new PopStateEvent('popstate'));

    render(<App />);

    expect(mocks.checkForUpdate).not.toHaveBeenCalled();
  });
});
