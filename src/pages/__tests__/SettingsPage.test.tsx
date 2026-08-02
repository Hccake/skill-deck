/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SettingsPage } from '../SettingsPage';
import type { AgentDefaultsSnapshot } from '@/stores/settings';

const ubuntu = { kind: 'wsl', distro_name: 'Ubuntu' } as const;
const mockLoadAgentDefaults = vi.fn();
const mockRefreshProjects = vi.fn();
let projectLoadState: 'idle' | 'ready' = 'ready';
const snapshot: AgentDefaultsSnapshot = {
  agents: [],
  selectionGroups: { global: [], project: [] },
  registryRevision: 'registry-1',
  defaults: { global: [], project: [] },
  loadState: 'ready' as const,
  loadRequestId: 1,
  saveRequestId: 0,
  saving: false,
  error: null,
};
let currentSnapshot = snapshot;

vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector({
    selectedContext: { environment: ubuntu, scope: { scope: 'global' } },
  }),
}));

vi.mock('@/stores/projects', () => ({
  useProjectStore: (selector: (state: unknown) => unknown) => selector({
    loadStateByEnvironment: { 'wsl:ubuntu': projectLoadState },
    refresh: mockRefreshProjects,
  }),
}));

vi.mock('@/stores/settings', () => ({
  useSettingsStore: (selector: (state: unknown) => unknown) => selector({
    agentDefaultsByEnvironment: { 'wsl:ubuntu': currentSnapshot },
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

vi.mock('@/components/settings/AgentSettingsPage', () => ({
  AgentSettingsPage: ({ context, view, agentId }: {
    context: { environment: typeof ubuntu };
    view?: string | null;
    agentId?: string | null;
  }) => (
    <div>
      agents-environment:{context.environment.distro_name};view:{view};id:{agentId}
    </div>
  ),
}));

vi.mock('@/components/settings/GeneralTab', () => ({
  GeneralTab: () => <div>general-section</div>,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe('SettingsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    projectLoadState = 'ready';
    currentSnapshot = snapshot;
  });

  it('does not load inactive Settings section data', async () => {
    projectLoadState = 'idle';
    currentSnapshot = { ...snapshot, loadState: 'idle' };

    render(
      <MemoryRouter initialEntries={['/settings']}>
        <SettingsPage />
      </MemoryRouter>,
    );

    expect(await screen.findByText('general-section')).toBeDefined();
    expect(mockRefreshProjects).not.toHaveBeenCalled();
    expect(mockLoadAgentDefaults).not.toHaveBeenCalled();
  });

  it('passes the selected environment snapshot to install preferences', async () => {
    render(
      <MemoryRouter initialEntries={['/settings?section=install-preferences']}>
        <SettingsPage />
      </MemoryRouter>,
    );

    expect(await screen.findByText('environment:Ubuntu;state:ready')).toBeDefined();
    await waitFor(() => expect(mockLoadAgentDefaults).not.toHaveBeenCalled());
  });

  it('opens Agent management as an independent settings subpage', async () => {
    render(
      <MemoryRouter initialEntries={['/settings?section=agents']}>
        <SettingsPage />
      </MemoryRouter>,
    );

    expect(await screen.findByText('agents-environment:Ubuntu;view:;id:')).toBeDefined();
  });

  it('parses the Agent child route once and passes it to the subpage', async () => {
    render(
      <MemoryRouter initialEntries={['/settings?section=agents&view=edit&id=my-agent']}>
        <SettingsPage />
      </MemoryRouter>,
    );

    expect(await screen.findByText('agents-environment:Ubuntu;view:edit;id:my-agent')).toBeDefined();
  });
});
