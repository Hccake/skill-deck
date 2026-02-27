// src/stores/__tests__/settings.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';

const mockGetLastSelectedAgents = vi.fn();
const mockSaveLastSelectedAgents = vi.fn();
const mockListAgents = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  getLastSelectedAgents: (...args: unknown[]) => mockGetLastSelectedAgents(...args),
  saveLastSelectedAgents: (...args: unknown[]) => mockSaveLastSelectedAgents(...args),
  listAgents: (...args: unknown[]) => mockListAgents(...args),
}));

import { useSettingsStore } from '../settings';

describe('useSettingsStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      defaultAgents: [],
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

  describe('toggleAgent', () => {
    it('adds agent when not selected', () => {
      useSettingsStore.setState({ defaultAgents: ['claude-code'] });
      mockSaveLastSelectedAgents.mockResolvedValue(undefined);

      useSettingsStore.getState().toggleAgent('opencode');

      expect(useSettingsStore.getState().defaultAgents).toContain('opencode');
      expect(useSettingsStore.getState().defaultAgents).toContain('claude-code');
    });

    it('removes agent when already selected', () => {
      useSettingsStore.setState({ defaultAgents: ['claude-code', 'opencode'] });
      mockSaveLastSelectedAgents.mockResolvedValue(undefined);

      useSettingsStore.getState().toggleAgent('opencode');

      expect(useSettingsStore.getState().defaultAgents).not.toContain('opencode');
      expect(useSettingsStore.getState().defaultAgents).toContain('claude-code');
    });

    it('rolls back on save failure', async () => {
      useSettingsStore.setState({ defaultAgents: ['claude-code'] });
      mockSaveLastSelectedAgents.mockRejectedValue(new Error('save failed'));

      useSettingsStore.getState().toggleAgent('opencode');

      expect(useSettingsStore.getState().defaultAgents).toContain('opencode');

      await vi.waitFor(() => {
        expect(useSettingsStore.getState().defaultAgents).toEqual(['claude-code']);
      });
    });
  });

  describe('isAgentSelected', () => {
    it('returns true for selected agent', () => {
      useSettingsStore.setState({ defaultAgents: ['claude-code'] });
      expect(useSettingsStore.getState().isAgentSelected('claude-code')).toBe(true);
    });

    it('returns false for unselected agent', () => {
      useSettingsStore.setState({ defaultAgents: ['claude-code'] });
      expect(useSettingsStore.getState().isAgentSelected('opencode')).toBe(false);
    });
  });
});
