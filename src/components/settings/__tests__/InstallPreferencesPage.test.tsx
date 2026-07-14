/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentInfo, EnvironmentRef } from '@/bindings';
import type { AgentDefaultsSnapshot } from '@/stores/settings';
import { InstallPreferencesPage } from '../InstallPreferencesPage';
import enLocale from '@/i18n/locales/en.json';
import { makeAgentScopeTarget } from '@/test-utils';
import { useMutationStore } from '@/stores/mutation';

const mockSaveAgentDefaults = vi.fn();

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
  }),
}));

const environment: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu' };
const agents: AgentInfo[] = [
  {
    id: 'amp',
    name: 'Amp',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.agents/skills',
    detected: true,
    targets: {
      global: makeAgentScopeTarget({ automatic: true, path: '~/.agents/skills' }),
      project: makeAgentScopeTarget({
        automatic: true,
        path: './.agents/skills',
        sharedPath: './.agents/skills',
      }),
    },
  },
  {
    id: 'claude-code',
    name: 'Claude Code',
    skillsDir: '.claude/skills',
    globalSkillsDir: '~/.claude/skills',
    detected: true,
    targets: {
      global: makeAgentScopeTarget({ automatic: false, path: '~/.claude/skills' }),
      project: makeAgentScopeTarget({
        automatic: false,
        path: './.claude/skills',
        sharedPath: './.agents/skills',
      }),
    },
  },
  {
    id: 'windsurf',
    name: 'Windsurf',
    skillsDir: '.windsurf/skills',
    globalSkillsDir: '~/.codeium/windsurf/skills',
    detected: false,
    targets: {
      global: makeAgentScopeTarget({ automatic: false, path: '~/.codeium/windsurf/skills' }),
      project: makeAgentScopeTarget({ automatic: false, path: './.windsurf/skills' }),
    },
  },
];

function snapshot(overrides: Partial<AgentDefaultsSnapshot> = {}): AgentDefaultsSnapshot {
  return {
    agents,
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
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
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
    const row = screen.getByText('Claude Code').closest('[role="button"]');
    expect(row?.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(row!);
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
    expect(screen.getByText('Claude Code').closest('[role="button"]')
      ?.getAttribute('aria-disabled')).toBe('true');
  });

  it('shows directly usable agents without nested selection', () => {
    renderPage();

    expect(screen.getAllByText('Amp').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Ready to use').length).toBeGreaterThan(0);
    expect(screen.getByText('Needs separate setup')).toBeDefined();
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
});
