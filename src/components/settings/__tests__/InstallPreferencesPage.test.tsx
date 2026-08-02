/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentRef, ResolvedAgent } from '@/bindings';
import type { AgentDefaultsSnapshot } from '@/stores/settings';
import { InstallPreferencesPage } from '../InstallPreferencesPage';
import enLocale from '@/i18n/locales/en.json';
import { makeResolvedAgent } from '@/test-utils';
import { useMutationStore } from '@/stores/mutation';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

const mockSaveAgentDefaults = vi.fn();
const mockLoadAgentDefaults = vi.fn();

function lookupLocaleKey(key: string): string | undefined {
  const segments = key.split('.');
  let cursor: unknown = enLocale;
  for (const segment of segments) {
    if (cursor && typeof cursor === 'object' && segment in (cursor as Record<string, unknown>)) {
      cursor = (cursor as Record<string, unknown>)[segment];
    } else {
      return undefined;
    }
  }
  return typeof cursor === 'string' ? cursor : undefined;
}

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      const value = lookupLocaleKey(key);
      if (value === undefined) return key;
      if (!options) return value;
      return value.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(options[name] ?? `{{${name}}}`));
    },
  }),
}));

vi.mock('@/stores/settings', () => ({
  useSettingsStore: (selector: (state: unknown) => unknown) => selector({
    saveAgentDefaults: mockSaveAgentDefaults,
    loadAgentDefaults: mockLoadAgentDefaults,
  }),
}));

const environment: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const agents: ResolvedAgent[] = [
  makeResolvedAgent({
    id: 'amp',
    displayName: 'Amp',
    global: { readsShared: true, sharedPath: '~/.agents/skills' },
    project: {
        readsShared: true,
        sharedPath: './.agents/skills',
    },
  }),
  makeResolvedAgent({
    id: 'claude-code',
    displayName: 'Claude Code',
    global: { readsShared: false, privatePath: '~/.claude/skills' },
    project: {
        readsShared: false,
        sharedPath: './.agents/skills',
        privatePath: './.claude/skills',
    },
  }),
  makeResolvedAgent({
    id: 'windsurf',
    displayName: 'Windsurf',
    detection: 'notDetected',
    global: {
      readsShared: false,
      privatePath: '~/.codeium/windsurf/skills',
    },
    project: {
      readsShared: false,
      privatePath: './.windsurf/skills',
    },
  }),
];

function snapshot(overrides: Partial<AgentDefaultsSnapshot> = {}): AgentDefaultsSnapshot {
  return {
    agents,
    selectionGroups: {
      global: [],
      project: [],
    },
    registryRevision: 'registry-1',
    defaults: { global: [], project: [] },
    loadState: 'ready',
    loadRequestId: 1,
    saveRequestId: 0,
    saving: false,
    error: null,
    ...overrides,
  };
}

function renderPage(selectedSnapshot = snapshot()) {
  return render(
    <InstallPreferencesPage environment={environment} snapshot={selectedSnapshot} />,
  );
}

describe('InstallPreferencesPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSaveAgentDefaults.mockResolvedValue(undefined);
    mockLoadAgentDefaults.mockResolvedValue(undefined);
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
    useInstallWizardSessionStore.setState({ revision: 0, active: false, loading: false });
  });

  it('saves the next defaults to the explicit environment', () => {
    renderPage(snapshot({
      defaults: { global: [], project: ['claude-code'] },
    }));

    fireEvent.click(screen.getAllByText('Claude Code')[0]);

    expect(mockSaveAgentDefaults).toHaveBeenCalledWith(environment, {
      global: ['claude-code'],
      project: ['claude-code'],
    });
  });

  it('disables changes while another mutation is active', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'install',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: false,
      },
    });

    renderPage();
    const row = screen.getByRole('checkbox', { name: /Claude Code/ });
    expect((row as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(row);
    expect(mockSaveAgentDefaults).not.toHaveBeenCalled();
  });

  it('disables changes while the install wizard is open', () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });

    renderPage();
    const row = screen.getByRole('checkbox', { name: /Claude Code/ });

    expect((row as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(row);
    expect(mockSaveAgentDefaults).not.toHaveBeenCalled();
  });

  it('disables changes while the selected environment is saving', () => {
    renderPage(snapshot({ saving: true }));

    fireEvent.click(screen.getAllByText('Claude Code')[0]);

    expect(mockSaveAgentDefaults).not.toHaveBeenCalled();
  });

  it('keeps cached rows visible while a refresh is loading', () => {
    renderPage(snapshot({ loadState: 'loading' }));

    expect(screen.getAllByText('Claude Code').length).toBeGreaterThan(0);
    expect((screen.getByRole('checkbox', { name: /Claude Code/ }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows directly usable agents without nested selection', () => {
    renderPage();

    expect(screen.getAllByText('Amp').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Ready to use').length).toBeGreaterThan(0);
    expect(screen.getByText('Needs separate setup')).toBeDefined();
  });

  it('shows an undetected user-defined Agent without opening the secondary list', () => {
    const customAgent = makeResolvedAgent({
      id: 'my-custom-agent',
      displayName: 'My Custom Agent',
      source: 'custom',
      detection: 'notDetected',
      global: {
        readsShared: true,
        sharedPath: '~/.agents/skills',
        privatePath: '~/.my-custom-agent/skills',
      },
      project: {
        readsShared: true,
        sharedPath: './.agents/skills',
        privatePath: './.my-custom-agent/skills',
      },
    });

    renderPage(snapshot({ agents: [customAgent] }));

    expect(screen.getByText('My Custom Agent')).toBeDefined();
  });

  it('keeps an indeterminate private-only Custom Agent selectable and reports its exact state', () => {
    const customAgent = makeResolvedAgent({
      id: 'private-custom-agent',
      displayName: 'Private Custom Agent',
      source: 'custom',
      detection: 'indeterminate',
      global: {
        readsShared: false,
        sharedPath: '~/.agents/skills',
        privatePath: '~/.private-custom-agent/skills',
      },
    });

    renderPage(snapshot({ agents: [customAgent] }));

    expect(screen.getByRole('checkbox', { name: /Private Custom Agent/ })).toBeDefined();
    expect(screen.getByText('Unable to determine')).toBeDefined();
    expect(screen.queryByText('Not detected')).toBeNull();
  });

  it('counts indeterminate shared readers separately from not-detected Agents', () => {
    const customAgent = makeResolvedAgent({
      id: 'shared-custom-agent',
      displayName: 'Shared Custom Agent',
      source: 'custom',
      detection: 'indeterminate',
      global: {
        readsShared: true,
        sharedPath: '~/.agents/skills',
      },
    });

    renderPage(snapshot({ agents: [customAgent] }));

    expect(screen.getByText('0 detected, 0 not detected, 1 unable to determine')).toBeDefined();
  });

  it('selects only detected additional agents when selecting all', () => {
    renderPage();

    fireEvent.click(screen.getAllByText('Select All')[0]);

    expect(mockSaveAgentDefaults).toHaveBeenCalledWith(environment, {
      global: ['claude-code'],
      project: [],
    });
  });

  it('uses global and project labels instead of workspace labels', () => {
    expect(enLocale.settings.installPreferences.globalTitle).toBe('Global');
    expect(enLocale.settings.installPreferences.projectTitle).toBe('Project');
    expect(enLocale.settings.installPreferences.description).not.toContain('workspaces');
  });

  it('saves all required Agent IDs represented by one Backend selection group', () => {
    renderPage(snapshot({
      agents: [agents[1], agents[2]],
      defaults: { global: ['claude-code'], project: [] },
      selectionGroups: {
        global: [{
          groupId: 'opaque-group',
          agentIds: ['claude-code', 'windsurf'],
        }],
        project: [],
      },
    }));

    const checkbox = screen.getByRole('checkbox', { name: /Claude Code.*Windsurf/i });
    expect(checkbox.getAttribute('aria-checked')).toBe('true');

    fireEvent.click(checkbox);

    expect(mockSaveAgentDefaults).toHaveBeenCalledWith(environment, {
      global: [],
      project: [],
    });
  });

  it('shows an explicit Retry action for load errors instead of a permanent Skeleton', () => {
    renderPage(snapshot({
      agents: [],
      loadState: 'error',
      error: { kind: 'custom', data: { message: 'load failed' } },
    }));

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    expect(mockLoadAgentDefaults).toHaveBeenCalledWith(environment);
  });

  it('keeps a stale save rollback visible until the user refreshes it', () => {
    renderPage(snapshot({
      loadState: 'stale',
      error: { kind: 'staleRegistry', data: {} } as never,
    }));

    expect(screen.getByRole('alert')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(mockLoadAgentDefaults).toHaveBeenCalledWith(environment);
  });
});
