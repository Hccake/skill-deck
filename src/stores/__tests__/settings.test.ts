// src/stores/__tests__/settings.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { AgentInfo } from '@/bindings';
import { makeAgentScopeTarget } from '@/test-utils';

const mockGetLastSelectedAgents = vi.fn();
const mockGetDefaultTargetAgents = vi.fn();
const mockSaveDefaultTargetAgents = vi.fn();
const mockListAgents = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  getLastSelectedAgents: (...args: unknown[]) => mockGetLastSelectedAgents(...args),
  getDefaultTargetAgents: (...args: unknown[]) => mockGetDefaultTargetAgents(...args),
  saveDefaultTargetAgents: (...args: unknown[]) => mockSaveDefaultTargetAgents(...args),
  listAgents: (...args: unknown[]) => mockListAgents(...args),
}));

import { useSettingsStore } from '../settings';

describe('useSettingsStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSaveDefaultTargetAgents.mockResolvedValue(undefined);
    useSettingsStore.setState({
      defaultTargetAgents: { global: [], project: [] },
      agentsLoaded: false,
    });
  });

  describe('theme', () => {
    it('toggles theme between light and dark', () => {
      useSettingsStore.setState({ theme: 'light' });
      useSettingsStore.getState().toggleTheme();
      expect(useSettingsStore.getState().theme).toBe('dark');

      useSettingsStore.getState().toggleTheme();
      expect(useSettingsStore.getState().theme).toBe('light');
    });

    it('sets specific theme', () => {
      useSettingsStore.getState().setTheme('dark');
      expect(useSettingsStore.getState().theme).toBe('dark');
    });
  });

  describe('locale', () => {
    it('sets locale', () => {
      useSettingsStore.getState().setLocale('zh-CN');
      expect(useSettingsStore.getState().locale).toBe('zh-CN');
    });
  });

  describe('scope-aware defaults', () => {
    const agents: AgentInfo[] = [
      {
        id: 'antigravity',
        name: 'Antigravity',
        skillsDir: '.agents/skills',
        globalSkillsDir: '~/.gemini/antigravity/skills',
        detected: true,
        targets: {
          global: makeAgentScopeTarget({
            automatic: false,
            path: '~/.gemini/antigravity/skills',
          }),
          project: makeAgentScopeTarget({
            automatic: true,
            path: '.agents/skills',
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
          global: makeAgentScopeTarget({
            automatic: false,
            path: '~/.claude/skills',
          }),
          project: makeAgentScopeTarget({
            automatic: false,
            path: '.claude/skills',
            sharedPath: './.agents/skills',
          }),
        },
      },
    ];

    it('loads persisted defaultTargetAgents when available', async () => {
      mockListAgents.mockResolvedValue(agents);
      mockGetDefaultTargetAgents.mockResolvedValue({
        global: ['antigravity', 'claude-code'],
        project: ['antigravity', 'claude-code'],
      });
      mockGetLastSelectedAgents.mockResolvedValue(['ignored']);

      await useSettingsStore.getState().loadDefaultTargetAgents();

      expect(useSettingsStore.getState().defaultTargetAgents).toEqual({
        global: ['antigravity', 'claude-code'],
        project: ['claude-code'],
      });
      expect(useSettingsStore.getState().agentsLoaded).toBe(true);
      expect(mockSaveDefaultTargetAgents).toHaveBeenCalledWith({
        global: ['antigravity', 'claude-code'],
        project: ['claude-code'],
      });
    });

    it('migrates lastSelectedAgents independently per scope', async () => {
      mockListAgents.mockResolvedValue(agents);
      mockGetDefaultTargetAgents.mockResolvedValue(null);
      mockGetLastSelectedAgents.mockResolvedValue(['antigravity', 'claude-code']);

      await useSettingsStore.getState().loadDefaultTargetAgents();

      expect(useSettingsStore.getState().defaultTargetAgents).toEqual({
        global: ['antigravity', 'claude-code'],
        project: ['claude-code'],
      });
    });

    it('falls back to lastSelectedAgents when scoped defaults fail to load', async () => {
      mockListAgents.mockResolvedValue(agents);
      mockGetDefaultTargetAgents.mockRejectedValue(new Error('read failed'));
      mockGetLastSelectedAgents.mockResolvedValue(['antigravity', 'claude-code']);

      await useSettingsStore.getState().loadDefaultTargetAgents();

      expect(useSettingsStore.getState().defaultTargetAgents).toEqual({
        global: ['antigravity', 'claude-code'],
        project: ['claude-code'],
      });
    });

    it('starts default target requests without waiting for agents to finish loading', async () => {
      let resolveAgents!: (value: AgentInfo[]) => void;
      mockListAgents.mockReturnValue(new Promise<AgentInfo[]>((resolve) => {
        resolveAgents = resolve;
      }));
      mockGetDefaultTargetAgents.mockResolvedValue({
        global: ['claude-code'],
        project: ['claude-code'],
      });
      mockGetLastSelectedAgents.mockResolvedValue([]);

      const loadPromise = useSettingsStore.getState().loadDefaultTargetAgents();

      await Promise.resolve();

      expect(mockListAgents).toHaveBeenCalledTimes(1);
      expect(mockGetDefaultTargetAgents).toHaveBeenCalledTimes(1);
      expect(mockGetLastSelectedAgents).toHaveBeenCalledTimes(1);

      resolveAgents(agents);
      await loadPromise;
    });

    it('saves one scope without losing the other scope', () => {
      mockSaveDefaultTargetAgents.mockResolvedValue(undefined);
      useSettingsStore.setState({
        allAgents: agents,
        defaultTargetAgents: {
          global: ['antigravity'],
          project: ['claude-code'],
        },
      });

      useSettingsStore.getState().setDefaultTargetAgents('global', ['claude-code']);

      expect(useSettingsStore.getState().defaultTargetAgents).toEqual({
        global: ['claude-code'],
        project: ['claude-code'],
      });
      expect(mockSaveDefaultTargetAgents).toHaveBeenCalledWith({
        global: ['claude-code'],
        project: ['claude-code'],
      });
    });

    it('filters default-available agents out of persisted scoped defaults', async () => {
      const scopedAgents: AgentInfo[] = [
        ...agents,
        {
          id: 'firebender',
          name: 'Firebender',
          skillsDir: '.agents/skills',
          globalSkillsDir: '~/.firebender/skills',
          detected: true,
          targets: {
            global: makeAgentScopeTarget({
              automatic: true,
              availability: 'shared-compatible',
              defaultAvailable: true,
              path: '~/.agents/skills',
              privatePath: '~/.firebender/skills',
            }),
            project: makeAgentScopeTarget({
              automatic: true,
              path: '.agents/skills',
              sharedPath: './.agents/skills',
            }),
          },
        },
      ];
      mockListAgents.mockResolvedValue(scopedAgents);
      mockGetDefaultTargetAgents.mockResolvedValue({
        global: ['firebender', 'claude-code'],
        project: ['firebender', 'claude-code'],
      });
      mockGetLastSelectedAgents.mockResolvedValue([]);

      await useSettingsStore.getState().loadDefaultTargetAgents();

      expect(useSettingsStore.getState().defaultTargetAgents).toEqual({
        global: ['claude-code'],
        project: ['claude-code'],
      });
      expect(mockSaveDefaultTargetAgents).toHaveBeenCalledWith({
        global: ['claude-code'],
        project: ['claude-code'],
      });
    });
  });

});
