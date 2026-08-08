/* @vitest-environment jsdom */

import '@/test-utils';
import { render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MainLayout from '../MainLayout';
import { createProjectWorkspace, type ProjectWorkspace } from '@/lib/projects/workspace';

const mocks = vi.hoisted(() => ({
  workspace: null as unknown as ProjectWorkspace,
  backendList: vi.fn().mockResolvedValue([]),
  now: 1_000,
  listen: vi.fn().mockResolvedValue(vi.fn()),
  environmentState: {
    environments: [{
      environment: { kind: 'native' as const },
      displayName: 'Windows',
      status: 'available' as const,
      revision: 1,
      error: null,
    }],
  },
  workspaceState: {
    selectedContext: {
      environment: { kind: 'native' as const },
      scope: { scope: 'global' as const },
    },
    transition: { kind: 'idle' as const },
  },
  skillsState: {
    refreshWorkspace: vi.fn(),
  },
}));

vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('react-router-dom', () => ({ Outlet: () => <div>outlet</div> }));
vi.mock('@/components/layout/Header', () => ({ Header: () => <div>header</div> }));
vi.mock('@/components/layout/InstallWizardSessionGate', () => ({
  InstallWizardSessionGate: ({ children }: React.PropsWithChildren) => children,
}));
vi.mock('@/components/layout/MutationStatusBar', () => ({ MutationStatusBar: () => null }));
vi.mock('@/hooks/useEnvironmentRuntimeMonitor', () => ({ useEnvironmentRuntimeMonitor: vi.fn() }));
vi.mock('@/hooks/useInstallWizardSessionMonitor', () => ({ useInstallWizardSessionMonitor: vi.fn() }));
vi.mock('@/stores/projects', () => ({
  get projectWorkspace() {
    return mocks.workspace;
  },
}));
vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: typeof mocks.environmentState) => unknown) => (
    selector(mocks.environmentState)
  ),
}));
vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: typeof mocks.workspaceState) => unknown) => (
    selector(mocks.workspaceState)
  ),
  selectWorkspaceTransitionActive: (state: typeof mocks.workspaceState) => (
    state.transition.kind !== 'idle'
  ),
}));
vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: typeof mocks.skillsState) => unknown) => (
    selector(mocks.skillsState)
  ),
}));

describe('MainLayout project catalog lifecycle', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listen.mockResolvedValue(vi.fn());
    mocks.now = 1_000;
    mocks.backendList.mockResolvedValue([]);
    mocks.workspace = createProjectWorkspace({
      backend: {
        list: mocks.backendList,
        add: vi.fn(),
        remove: vi.fn(),
        setCrossStorageWarning: vi.fn(),
      },
      environment: {
        isAvailable: () => true,
        revision: () => 1,
        ensureAvailable: async () => undefined,
      },
      catalogObserver: {
        captureContext: () => ({
          context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
          revision: 0,
        }),
        onCompleteSnapshot: () => undefined,
      },
      write: {
        run: async <T,>(operation: () => Promise<T>) => ({
          status: 'succeeded' as const,
          value: await operation(),
        }),
      },
      now: () => mocks.now,
    });
  });

  it('uses one Backend read across page mount and a fresh-window focus', async () => {
    const view = render(<MainLayout />);

    await vi.waitFor(() => expect(mocks.backendList).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(mocks.workspace.getSnapshot({ kind: 'native' }).completeness)
      .toBe('complete'));
    expect(mocks.backendList).toHaveBeenCalledWith({ kind: 'native' });
    view.rerender(<MainLayout />);
    expect(mocks.backendList).toHaveBeenCalledOnce();

    window.dispatchEvent(new Event('focus'));
    await Promise.resolve();
    expect(mocks.backendList).toHaveBeenCalledOnce();

    mocks.now += 5 * 60 * 1_000 + 1;
    window.dispatchEvent(new Event('focus'));
    await vi.waitFor(() => expect(mocks.backendList).toHaveBeenCalledTimes(2));
  });
});
