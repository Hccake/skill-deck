/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import App from '../App';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

const mocks = vi.hoisted(() => ({
  refreshWorkspace: vi.fn(),
  listen: vi.fn().mockResolvedValue(() => undefined),
  monitorEnvironmentRuntime: vi.fn(),
  monitorInstallWizardSession: vi.fn(),
  recoveryCenter: vi.fn(),
  checkForUpdate: vi.fn(),
  shouldAutoCheck: vi.fn(() => false),
  projectWorkspaceExecute: vi.fn().mockResolvedValue({ status: 'succeeded' }),
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
vi.mock('@/pages/SkillsPage', () => ({ SkillsPage: () => <div>skills-page</div> }));
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
vi.mock('@/stores/projects', () => ({
  projectWorkspace: { execute: (...args: unknown[]) => mocks.projectWorkspaceExecute(...args) },
}));
vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    environments: [{
      environment: { kind: 'native' },
      displayName: 'Windows',
      status: 'available',
      revision: 1,
      error: null,
    }],
  }),
}));
vi.mock('@/stores/updater', () => ({
  useUpdaterStore: Object.assign((selector: (state: unknown) => unknown) => selector({
    status: 'idle',
    dialogVisible: false,
    checkForUpdate: mocks.checkForUpdate,
    shouldAutoCheck: mocks.shouldAutoCheck,
  }), { getState: () => ({ error: null }) }),
}));
vi.mock('@/stores/workspace-context', () => {
  const state = {
      selectedContext: {
        environment: { kind: 'native' },
        scope: { scope: 'global' },
      },
      transition: { kind: 'idle' },
    };
  const useWorkspaceContextStore = Object.assign(
    (selector: (current: typeof state) => unknown) => selector(state),
    { getState: () => state },
  );
  return {
    useWorkspaceContextStore,
    selectWorkspaceTransitionActive: (current: typeof state) => current.transition.kind !== 'idle',
  };
});

describe('App', () => {
  beforeEach(() => {
    mocks.refreshWorkspace.mockClear();
    mocks.listen.mockClear();
    mocks.monitorEnvironmentRuntime.mockClear();
    mocks.monitorInstallWizardSession.mockClear();
    mocks.recoveryCenter.mockClear();
    mocks.checkForUpdate.mockClear();
    mocks.projectWorkspaceExecute.mockClear();
    mocks.shouldAutoCheck.mockReset();
    mocks.shouldAutoCheck.mockReturnValue(false);
    mocks.wizardResultHandler = null;
    useInstallWizardSessionStore.setState({
      revision: 0,
      active: false,
      loading: false,
      hasConfirmedSnapshot: true,
      syncError: null,
    });
    window.history.replaceState({}, '', '/');
  });

  it('refreshes the committed Native Global workspace after the wizard completes', async () => {
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

  it('keeps route content behind the initial install wizard session check', async () => {
    useInstallWizardSessionStore.setState({ loading: true, hasConfirmedSnapshot: false });

    render(<App />);

    const startupRegion = await screen.findByRole('main');
    expect(startupRegion.className).toContain('flex-1');
    expect(startupRegion.className).toContain('overflow-hidden');
    await waitFor(() => expect(screen.queryByText('common.loading')).toBeNull());
    expect(screen.queryByText('skills-page')).toBeNull();

    act(() => useInstallWizardSessionStore.setState({
      loading: false,
      hasConfirmedSnapshot: true,
    }));
    expect(await screen.findByText('skills-page')).toBeDefined();
    const contentRegion = screen.getByRole('main');
    expect(contentRegion.className).toContain('flex-1');
    expect(contentRegion.className).toContain('overflow-hidden');
  });

  it('keeps the same content region while an inactive session refreshes', async () => {
    render(<App />);
    expect(await screen.findByText('skills-page')).toBeDefined();
    const contentRegion = screen.getByRole('main');

    act(() => useInstallWizardSessionStore.setState({ loading: true }));

    expect(screen.getByRole('main')).toBe(contentRegion);
    expect(screen.queryByText('installWizardSession.startupDescription')).toBeNull();
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
