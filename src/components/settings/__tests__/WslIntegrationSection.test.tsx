/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppError, SkillLocationRef } from '@/bindings';
import { WslIntegrationSection } from '../WslIntegrationSection';

const mocks = vi.hoisted(() => ({
  supported: false,
  enabled: false,
  transition: { kind: 'idle' } as { kind: string; phase?: string },
  failure: null as { stage: string; error: AppError } | null,
  selectedContext: {
    environment: { kind: 'native' },
    scope: { scope: 'global' },
  } as SkillLocationRef,
  writeBlocked: false,
  changeWslIntegration: vi.fn(async (_enabled: boolean) => ({ status: 'succeeded' as const })),
  clearFailure: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    wslIntegrationSupported: mocks.supported,
    wslIntegrationEnabled: mocks.enabled,
  }),
}));
vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector({
    selectedContext: mocks.selectedContext,
    transition: mocks.transition,
    wslIntegrationFailure: mocks.failure,
    changeWslIntegration: mocks.changeWslIntegration,
    clearWslIntegrationFailure: mocks.clearFailure,
  }),
}));
vi.mock('@/hooks/useBusinessWriteBlocked', () => ({
  useBusinessWriteBlocked: () => mocks.writeBlocked,
}));

describe('WslIntegrationSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.supported = false;
    mocks.enabled = false;
    mocks.transition = { kind: 'idle' };
    mocks.failure = null;
    mocks.selectedContext = {
      environment: { kind: 'native' },
      scope: { scope: 'global' },
    };
    mocks.writeBlocked = false;
    mocks.changeWslIntegration.mockResolvedValue({ status: 'succeeded' });
  });

  it('does not render outside Windows', () => {
    render(<WslIntegrationSection />);

    expect(screen.queryByRole('switch')).toBeNull();
    expect(screen.queryByText('settings.general.wslTitle')).toBeNull();
  });

  it('enables WSL integration directly from Native', async () => {
    mocks.supported = true;
    render(<WslIntegrationSection />);

    fireEvent.click(screen.getByRole('switch', { name: 'settings.general.wslTitle' }));

    await waitFor(() => expect(mocks.changeWslIntegration).toHaveBeenCalledWith(true));
  });

  it('switches to Native before disabling the active WSL environment', async () => {
    mocks.supported = true;
    mocks.enabled = true;
    mocks.selectedContext = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    render(<WslIntegrationSection />);

    fireEvent.click(screen.getByRole('switch', { name: 'settings.general.wslTitle' }));
    expect(screen.getByRole('alertdialog')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'settings.general.wslDisableConfirm' }));

    await waitFor(() => expect(mocks.changeWslIntegration).toHaveBeenCalledWith(false));
  });

  it('keeps the setting read-only while business writes or Environment switching are active', () => {
    mocks.supported = true;
    mocks.writeBlocked = true;
    const { rerender } = render(<WslIntegrationSection />);

    expect((screen.getByRole('switch') as HTMLButtonElement).disabled).toBe(true);

    mocks.writeBlocked = false;
    mocks.transition = { kind: 'switchEnvironment' };
    rerender(<WslIntegrationSection />);
    expect((screen.getByRole('switch') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows progress and prevents repeated changes while the workflow is pending', () => {
    mocks.supported = true;
    mocks.transition = { kind: 'wslIntegration', phase: 'enabling' };
    render(<WslIntegrationSection />);

    const status = screen.getByRole('status', { name: 'settings.general.wslSaving' });
    expect(status).toBeTruthy();
    expect(status.getAttribute('class')).toContain('motion-reduce:animate-none');
    expect((screen.getByRole('switch') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows a setting error in the confirmation dialog when disabling fails', async () => {
    mocks.supported = true;
    mocks.enabled = true;
    mocks.selectedContext = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    const view = render(<WslIntegrationSection />);

    fireEvent.click(screen.getByRole('switch', { name: 'settings.general.wslTitle' }));
    mocks.failure = {
      stage: 'busy',
      error: { kind: 'wslIntegrationBusy', data: { reason: 'installWizard' } },
    };
    view.rerender(<WslIntegrationSection />);

    expect((await screen.findByRole('alert')).textContent).toBe(
      'settings.general.wslBusyInstallWizard',
    );
    expect(screen.getByRole('alertdialog')).toBeTruthy();
  });

  it('does not offer another Native switch after only the setting write failed', async () => {
    mocks.supported = true;
    mocks.enabled = true;
    mocks.selectedContext = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    const view = render(<WslIntegrationSection />);

    fireEvent.click(screen.getByRole('switch', { name: 'settings.general.wslTitle' }));
    mocks.selectedContext = {
      environment: { kind: 'native' },
      scope: { scope: 'global' },
    };
    mocks.failure = {
      stage: 'persistSetting',
      error: { kind: 'custom', data: { message: 'write failed' } },
    };
    view.rerender(<WslIntegrationSection />);

    expect(screen.getByText('settings.general.wslDisableAfterNativeDescription')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'settings.general.wslDisableOnlyConfirm' }))
      .toBeTruthy();
    expect(screen.queryByText('settings.general.wslDisableDescription')).toBeNull();
  });

  it('shows the active disable phase inside the confirmation dialog', () => {
    mocks.supported = true;
    mocks.enabled = true;
    mocks.selectedContext = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    const view = render(<WslIntegrationSection />);
    fireEvent.click(screen.getByRole('switch', { name: 'settings.general.wslTitle' }));
    mocks.selectedContext = {
      environment: { kind: 'native' },
      scope: { scope: 'global' },
    };
    mocks.transition = { kind: 'wslIntegration', phase: 'disabling' };
    view.rerender(<WslIntegrationSection />);

    expect(screen.getByRole('status', { name: 'settings.general.wslDisabling' })).toBeTruthy();
  });
});
