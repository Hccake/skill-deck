/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SettingsPage } from '../SettingsPage';

const ubuntu = { kind: 'wsl', distro_name: 'Ubuntu' } as const;
const mockRefreshProjects = vi.fn();
let projectLoadState: 'idle' | 'ready' = 'ready';

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
  });

  it('does not load inactive Settings section data', async () => {
    projectLoadState = 'idle';

    render(
      <MemoryRouter initialEntries={['/settings']}>
        <SettingsPage />
      </MemoryRouter>,
    );

    expect(await screen.findByText('general-section')).toBeDefined();
    expect(mockRefreshProjects).not.toHaveBeenCalled();
  });

  it('hides install preferences and falls back from its legacy section URL', async () => {
    render(
      <MemoryRouter initialEntries={['/settings?section=install-preferences']}>
        <SettingsPage />
      </MemoryRouter>,
    );

    expect(await screen.findByText('general-section')).toBeDefined();
    expect(screen.queryByRole('button', {
      name: 'settings.nav.installPreferences',
    })).toBeNull();
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
