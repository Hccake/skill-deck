/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { AppError, ContextRef, EnvironmentInfo, EnvironmentRef } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { TooltipProvider } from '@/components/ui/tooltip';
import { GlobalEnvironmentSwitcher } from '../GlobalEnvironmentSwitcher';

const host: EnvironmentInfo = {
  environment: { kind: 'host' },
  displayName: 'Windows',
  status: 'available',
  revision: 1,
  error: null,
};
const ubuntu: EnvironmentInfo = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  displayName: 'Ubuntu',
  status: 'available',
  revision: 1,
  error: null,
};

const mocks = vi.hoisted(() => ({
  environments: [] as EnvironmentInfo[],
  discoveryError: null as AppError | null,
  selectedContext: {
    environment: { kind: 'host' },
    scope: { scope: 'global' },
  } as ContextRef,
  transition: { kind: 'idle' } as { kind: string; target?: EnvironmentRef },
  switchEnvironment: vi.fn(async (_environment: EnvironmentRef) => undefined),
  discover: vi.fn(async () => undefined),
  guard: vi.fn(async (action: () => void | Promise<void>) => {
    await action();
    return true;
  }),
  toastError: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => {
      if (key === 'context.environmentWslName') return `WSL · ${values?.environment}`;
      if (key === 'context.environmentMenuLabel') return `Environment: ${values?.environment}`;
      return key;
    },
  }),
}));
vi.mock('sonner', () => ({ toast: { error: mocks.toastError } }));
vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: EnvironmentRef) => (
    environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name.toLocaleLowerCase()}`
  ),
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    environments: mocks.environments,
    discoveryError: mocks.discoveryError,
    discover: mocks.discover,
  }),
}));
vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector({
    selectedContext: mocks.selectedContext,
    transition: mocks.transition,
    switchEnvironment: mocks.switchEnvironment,
  }),
}));
vi.mock('@/lifecycle/unsaved-changes-context', () => ({
  useOptionalUnsavedChanges: () => ({ guard: mocks.guard }),
}));

function openEnvironmentMenu() {
  fireEvent.pointerDown(screen.getByRole('button', { name: /Environment:/ }), {
    button: 0,
    ctrlKey: false,
  });
}

function renderSwitcher() {
  return render(
    <TooltipProvider>
      <GlobalEnvironmentSwitcher />
    </TooltipProvider>,
  );
}

describe('GlobalEnvironmentSwitcher', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.environments = [host];
    mocks.discoveryError = null;
    mocks.selectedContext = {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    };
    mocks.transition = { kind: 'idle' };
    mocks.switchEnvironment.mockResolvedValue(undefined);
    mocks.discover.mockResolvedValue(undefined);
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      cancelling: false,
      loading: false,
    });
    useInstallWizardSessionStore.setState({ revision: 0, active: false, loading: false });
  });

  it('stays hidden when Host is the only available Environment', () => {
    renderSwitcher();

    expect(screen.queryByRole('button', { name: /Environment:/ })).toBeNull();
  });

  it('shows the WSL type with the distro name and switches through the unsaved-change guard', async () => {
    mocks.environments = [host, ubuntu];
    mocks.selectedContext = {
      environment: ubuntu.environment,
      scope: { scope: 'global' },
    };
    renderSwitcher();

    const trigger = screen.getByRole('button', { name: 'Environment: WSL · Ubuntu' });
    expect(trigger.querySelector('[data-environment-platform="wsl"]')).not.toBeNull();
    openEnvironmentMenu();
    const windowsItem = await screen.findByRole('menuitemradio', { name: 'Windows' });
    expect(windowsItem.querySelector('[data-environment-platform="host"]')).not.toBeNull();
    fireEvent.click(windowsItem);

    await waitFor(() => expect(mocks.guard).toHaveBeenCalledTimes(1));
    expect(mocks.switchEnvironment).toHaveBeenCalledWith(host.environment);
  });

  it('sizes the menu to its content without showing leading radio dots', async () => {
    mocks.environments = [host, ubuntu];
    renderSwitcher();

    openEnvironmentMenu();

    const menu = await screen.findByRole('menu');
    expect(menu.className).toContain('w-max');
    expect(menu.className).not.toContain('w-64');
    for (const item of screen.getAllByRole('menuitemradio')) {
      expect(item.className).toContain('pl-2');
      expect(item.className).toContain('[&>span:first-child]:hidden');
    }
  });

  it.each([
    ['an Environment connection is pending', ubuntu.environment, null],
    ['a write operation is active', null, {
      id: 'mutation-1',
      kind: 'install' as const,
      context: { environment: { kind: 'host' as const }, scope: { scope: 'global' as const } },
      phase: 'preparing' as const,
      progress: null,
      cancelable: true,
    }],
  ])('disables global switching while %s', (_label, pendingEnvironment, activeMutation) => {
    mocks.environments = [host, ubuntu];
    mocks.transition = pendingEnvironment
      ? { kind: 'switchEnvironment', target: pendingEnvironment }
      : { kind: 'idle' };
    useMutationStore.setState({ activeMutation });

    renderSwitcher();

    expect((screen.getByRole('button', { name: /Environment:/ }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it('keeps discovery failure and retry in the global Environment menu', async () => {
    mocks.discoveryError = {
      kind: 'custom',
      data: { message: 'wsl discovery failed' },
    };
    renderSwitcher();

    openEnvironmentMenu();
    expect(await screen.findByText('context.environmentDiscoveryFailed')).toBeDefined();
    fireEvent.click(screen.getByRole('menuitem', { name: 'context.environmentRetry' }));

    await waitFor(() => expect(mocks.discover).toHaveBeenCalledTimes(1));
  });

  it('reports a failed switch once from the global control', async () => {
    const error: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution stopped' },
    };
    mocks.environments = [host, ubuntu];
    mocks.switchEnvironment.mockRejectedValue(error);
    renderSwitcher();

    openEnvironmentMenu();
    fireEvent.click(await screen.findByRole('menuitemradio', { name: 'WSL · Ubuntu' }));

    await waitFor(() => expect(mocks.toastError).toHaveBeenCalledTimes(1));
    expect(mocks.toastError).toHaveBeenCalledWith('addSkill.error.environmentUnavailable');
  });

  it('keeps Environment switching available while the install wizard makes business actions read-only', () => {
    mocks.environments = [host, ubuntu];
    useInstallWizardSessionStore.setState({ revision: 1, active: true });

    renderSwitcher();

    expect((screen.getByRole('button', { name: /Environment:/ }) as HTMLButtonElement).disabled)
      .toBe(false);
  });
});
