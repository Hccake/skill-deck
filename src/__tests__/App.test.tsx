/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import App from '../App';

const mocks = vi.hoisted(() => ({
  discoverEnvironments: vi.fn(),
  listen: vi.fn().mockResolvedValue(() => undefined),
  requestClose: vi.fn().mockResolvedValue('performed'),
}));

vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
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
vi.mock('@/pages/SkillsPage', () => ({ SkillsPage: () => null }));
vi.mock('@/pages/DiscoverPage', () => ({ DiscoverPage: () => null }));
vi.mock('@/pages/SettingsPage', () => ({ SettingsPage: () => null }));
vi.mock('@/pages/WizardPage', () => ({ WizardPage: () => null }));
vi.mock('@/components/ui/sonner', () => ({ Toaster: () => null }));
vi.mock('@/components/ui/tooltip', () => ({ TooltipProvider: ({ children }: { children: React.ReactNode }) => children }));
vi.mock('@/components/update-dialog', () => ({ UpdateDialog: () => null }));
vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: { fetchSkills: () => void }) => unknown) => selector({ fetchSkills: vi.fn() }),
}));
vi.mock('@/stores/updater', () => ({
  useUpdaterStore: Object.assign(
    () => ({ status: 'idle', checkForUpdate: vi.fn(), shouldAutoCheck: () => false }),
    { getState: () => ({ error: null }) },
  ),
}));
vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: { discoverEnvironments: () => Promise<void> }) => unknown) => selector({
    discoverEnvironments: mocks.discoverEnvironments,
  }),
}));

describe('App', () => {
  it('discovers environments whenever the main application starts', async () => {
    mocks.discoverEnvironments.mockResolvedValue(undefined);

    render(<App />);

    await waitFor(() => expect(mocks.discoverEnvironments).toHaveBeenCalledTimes(1));
  });

  it('mounts the global mutation status in the main window', () => {
    render(<App />);

    expect(screen.getByText('mutation-status-bar')).toBeDefined();
  });

  it('mounts close protection in the main window', () => {
    render(<App />);

    expect(screen.getByText('close-protection-dialog')).toBeDefined();
  });
});
