/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import App from '../App';

const mocks = vi.hoisted(() => ({
  discoverEnvironments: vi.fn(),
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@/components/layout/Header', () => ({ Header: () => null }));
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
});
