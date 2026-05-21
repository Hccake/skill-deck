/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentInfo } from '@/hooks/useTauriApi';
import { InstallPreferencesPage } from '../InstallPreferencesPage';
import enLocale from '@/i18n/locales/en.json';

const mockToggleDefaultTargetAgent = vi.fn();
const mockSetDefaultTargetAgents = vi.fn();

const mockSettingsState = vi.hoisted(() => ({
  allAgents: [] as AgentInfo[],
  agentsLoaded: true,
  defaultTargetAgents: {
    global: [] as string[],
    project: [] as string[],
  },
}));

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
      if (options && typeof options === 'object') {
        return value.replace(/\{\{(\w+)\}\}/g, (_, name: string) => {
          const replacement = options[name];
          return replacement === undefined ? `{{${name}}}` : String(replacement);
        });
      }
      return value;
    },
  }),
}));

vi.mock('@/stores/settings', () => ({
  useSettingsStore: () => ({
    ...mockSettingsState,
    toggleDefaultTargetAgent: mockToggleDefaultTargetAgent,
    setDefaultTargetAgents: mockSetDefaultTargetAgents,
  }),
}));

const agents: AgentInfo[] = [
  {
    id: 'amp',
    name: 'Amp',
    skillsDir: '.agents/skills',
    globalSkillsDir: '~/.agents/skills',
    detected: true,
    targets: {
      global: {
        supported: true,
        automatic: true,
        path: '~/.agents/skills',
      },
      project: {
        supported: true,
        automatic: true,
        path: './.agents/skills',
      },
    },
  },
  {
    id: 'claude-code',
    name: 'Claude Code',
    skillsDir: '.claude/skills',
    globalSkillsDir: '~/.claude/skills',
    detected: true,
    targets: {
      global: {
        supported: true,
        automatic: false,
        path: '~/.claude/skills',
      },
      project: {
        supported: true,
        automatic: false,
        path: './.claude/skills',
      },
    },
  },
  {
    id: 'windsurf',
    name: 'Windsurf',
    skillsDir: '.windsurf/skills',
    globalSkillsDir: '~/.codeium/windsurf/skills',
    detected: false,
    targets: {
      global: {
        supported: true,
        automatic: false,
        path: '~/.codeium/windsurf/skills',
      },
      project: {
        supported: true,
        automatic: false,
        path: './.windsurf/skills',
      },
    },
  },
];

describe('InstallPreferencesPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSettingsState.allAgents = agents;
    mockSettingsState.agentsLoaded = true;
    mockSettingsState.defaultTargetAgents = {
      global: [],
      project: [],
    };
  });

  it('shows automatically included agents without requiring a nested expansion', () => {
    render(<InstallPreferencesPage />);

    expect(screen.getAllByText('Amp').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Available automatically').length).toBeGreaterThan(0);
    expect(screen.getByText(/shared directory/)).toBeDefined();
  });

  it('uses global and project labels instead of workspace labels', () => {
    expect(enLocale.settings.installPreferences.globalTitle).toBe('Global');
    expect(enLocale.settings.installPreferences.projectTitle).toBe('Project');
    expect(enLocale.settings.installPreferences.description).toContain('Global and project defaults');
    expect(enLocale.settings.installPreferences.description).not.toContain('workspaces');
    expect(Object.hasOwn(enLocale.settings.installPreferences, 'globalAutomaticPath')).toBe(false);
    expect(Object.hasOwn(enLocale.settings.installPreferences, 'projectAutomaticPath')).toBe(false);
  });

  it('toggles an additional global agent from the default target list', () => {
    render(<InstallPreferencesPage />);

    fireEvent.click(screen.getAllByText('Claude Code')[0]);

    expect(mockToggleDefaultTargetAgent).toHaveBeenCalledWith('global', 'claude-code');
  });

  it('selects only detected additional agents when selecting all', () => {
    render(<InstallPreferencesPage />);

    fireEvent.click(screen.getAllByText('Select All')[0]);

    expect(mockSetDefaultTargetAgents).toHaveBeenCalledWith('global', ['claude-code']);
  });
});
