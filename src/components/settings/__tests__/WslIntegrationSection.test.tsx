/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, EnvironmentRef } from '@/bindings';
import { WslIntegrationSection } from '../WslIntegrationSection';

const mocks = vi.hoisted(() => ({
  supported: false,
  enabled: false,
  pendingEnvironment: null as EnvironmentRef | null,
  selectedContext: {
    environment: { kind: 'host' },
    scope: { scope: 'global' },
  } as ContextRef,
  writeBlocked: false,
  setEnabled: vi.fn(async (_enabled: boolean): Promise<void> => undefined),
  switchEnvironment: vi.fn(async (_environment: EnvironmentRef) => undefined),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    wslIntegrationSupported: mocks.supported,
    wslIntegrationEnabled: mocks.enabled,
    setWslIntegrationEnabled: mocks.setEnabled,
  }),
}));
vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector({
    selectedContext: mocks.selectedContext,
    pendingEnvironment: mocks.pendingEnvironment,
    switchEnvironment: mocks.switchEnvironment,
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
    mocks.pendingEnvironment = null;
    mocks.selectedContext = {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    };
    mocks.writeBlocked = false;
    mocks.setEnabled.mockResolvedValue(undefined);
    mocks.switchEnvironment.mockResolvedValue(undefined);
  });

  it('does not render outside Windows', () => {
    render(<WslIntegrationSection />);

    expect(screen.queryByRole('switch')).toBeNull();
    expect(screen.queryByText('settings.general.wslTitle')).toBeNull();
  });

  it('enables WSL integration directly from Host', async () => {
    mocks.supported = true;
    render(<WslIntegrationSection />);

    fireEvent.click(screen.getByRole('switch', { name: 'settings.general.wslTitle' }));

    await waitFor(() => expect(mocks.setEnabled).toHaveBeenCalledWith(true));
    expect(mocks.switchEnvironment).not.toHaveBeenCalled();
  });

  it('switches to Host before disabling the active WSL environment', async () => {
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

    await waitFor(() => expect(mocks.setEnabled).toHaveBeenCalledWith(false));
    expect(mocks.switchEnvironment).toHaveBeenCalledWith({ kind: 'host' });
    expect(mocks.switchEnvironment.mock.invocationCallOrder[0])
      .toBeLessThan(mocks.setEnabled.mock.invocationCallOrder[0]);
  });

  it('keeps the setting read-only while business writes or Environment switching are active', () => {
    mocks.supported = true;
    mocks.writeBlocked = true;
    const { rerender } = render(<WslIntegrationSection />);

    expect((screen.getByRole('switch') as HTMLButtonElement).disabled).toBe(true);

    mocks.writeBlocked = false;
    mocks.pendingEnvironment = { kind: 'wsl', distro_name: 'Ubuntu' };
    rerender(<WslIntegrationSection />);
    expect((screen.getByRole('switch') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows progress and prevents repeated changes while discovery is pending', async () => {
    mocks.supported = true;
    let finish: (() => void) | undefined;
    mocks.setEnabled.mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
    render(<WslIntegrationSection />);

    fireEvent.click(screen.getByRole('switch', { name: 'settings.general.wslTitle' }));

    expect(screen.getByRole('status', { name: 'settings.general.wslSaving' })).toBeTruthy();
    expect((screen.getByRole('switch') as HTMLButtonElement).disabled).toBe(true);
    finish?.();
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull());
  });

  it('shows a stable error when the setting change fails', async () => {
    mocks.supported = true;
    mocks.setEnabled.mockRejectedValue(new Error('save failed'));
    render(<WslIntegrationSection />);

    fireEvent.click(screen.getByRole('switch', { name: 'settings.general.wslTitle' }));

    expect(await screen.findByText('settings.general.wslSaveError')).toBeTruthy();
  });
});
