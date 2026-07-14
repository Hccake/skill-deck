/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SettingsPage } from '../SettingsPage';

const ubuntu = { kind: 'wsl', distro_name: 'Ubuntu' } as const;
const mockLoadAgentDefaults = vi.fn();
const snapshot = {
  agents: [],
  defaults: { global: [], project: [] },
  loadState: 'ready' as const,
  loadRequestId: 1,
  saveRequestId: 0,
  saving: false,
  error: null,
};

vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector({
    selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
  }),
}));

vi.mock('@/stores/projects', () => ({
  useProjectStore: (selector: (state: unknown) => unknown) => selector({
    loadStateByEnvironment: { 'wsl:Ubuntu': 'ready' },
    refresh: vi.fn(),
  }),
}));

vi.mock('@/stores/settings', () => ({
  useSettingsStore: (selector: (state: unknown) => unknown) => selector({
    agentDefaultsByEnvironment: { 'wsl:Ubuntu': snapshot },
    loadAgentDefaults: mockLoadAgentDefaults,
  }),
}));

vi.mock('@/components/settings/InstallPreferencesPage', () => ({
  InstallPreferencesPage: ({ environment, snapshot: selectedSnapshot }: {
    environment: typeof ubuntu;
    snapshot: typeof snapshot;
  }) => (
    <div>
      environment:{environment.distro_name};state:{selectedSnapshot.loadState}
    </div>
  ),
}));

describe('SettingsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('passes the selected environment snapshot to install preferences', async () => {
    render(
      <MemoryRouter initialEntries={['/settings?section=install-preferences']}>
        <SettingsPage />
      </MemoryRouter>,
    );

    expect(screen.getByText('environment:Ubuntu;state:ready')).toBeDefined();
    await waitFor(() => expect(mockLoadAgentDefaults).not.toHaveBeenCalled());
  });
});
