// src/stores/__tests__/context.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useContextStore } from '../context';

const mockGetConfig = vi.fn();
const mockAddProject = vi.fn();
const mockRemoveProject = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  getConfig: (...args: unknown[]) => mockGetConfig(...args),
  addProject: (...args: unknown[]) => mockAddProject(...args),
  removeProject: (...args: unknown[]) => mockRemoveProject(...args),
}));

describe('useContextStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useContextStore.setState({
      selectedContext: 'global',
      projects: [],
      projectsLoaded: false,
    });
  });

  describe('selectContext', () => {
    it('sets selectedContext to a project path', () => {
      useContextStore.getState().selectContext('/my/project');
      expect(useContextStore.getState().selectedContext).toBe('/my/project');
    });

    it('sets selectedContext back to global', () => {
      useContextStore.getState().selectContext('/my/project');
      useContextStore.getState().selectContext('global');
      expect(useContextStore.getState().selectedContext).toBe('global');
    });
  });

  describe('toggleProjectContext', () => {
    it('selects project when global is active', () => {
      useContextStore.getState().toggleProjectContext('/my/project');
      expect(useContextStore.getState().selectedContext).toBe('/my/project');
    });

    it('deselects project back to global when same project is toggled', () => {
      useContextStore.getState().selectContext('/my/project');
      useContextStore.getState().toggleProjectContext('/my/project');
      expect(useContextStore.getState().selectedContext).toBe('global');
    });

    it('switches to different project when another is active', () => {
      useContextStore.getState().selectContext('/project-a');
      useContextStore.getState().toggleProjectContext('/project-b');
      expect(useContextStore.getState().selectedContext).toBe('/project-b');
    });
  });

  describe('loadProjects', () => {
    it('loads projects from backend config', async () => {
      mockGetConfig.mockResolvedValue({ projects: ['/a', '/b'] });
      await useContextStore.getState().loadProjects();
      expect(useContextStore.getState().projects).toEqual(['/a', '/b']);
      expect(useContextStore.getState().projectsLoaded).toBe(true);
    });

    it('sets empty projects on error', async () => {
      mockGetConfig.mockRejectedValue(new Error('network error'));
      await useContextStore.getState().loadProjects();
      expect(useContextStore.getState().projects).toEqual([]);
      expect(useContextStore.getState().projectsLoaded).toBe(true);
    });
  });

  describe('removeProject', () => {
    it('resets to global when removing the selected project', async () => {
      useContextStore.setState({ selectedContext: '/my/project' });
      mockRemoveProject.mockResolvedValue([]);
      await useContextStore.getState().removeProject('/my/project');
      expect(useContextStore.getState().selectedContext).toBe('global');
      expect(useContextStore.getState().projects).toEqual([]);
    });

    it('keeps current selection when removing a different project', async () => {
      useContextStore.setState({ selectedContext: '/project-a' });
      mockRemoveProject.mockResolvedValue(['/project-a']);
      await useContextStore.getState().removeProject('/project-b');
      expect(useContextStore.getState().selectedContext).toBe('/project-a');
    });
  });

  describe('addProject', () => {
    it('updates projects list after adding', async () => {
      mockAddProject.mockResolvedValue(['/a', '/b']);
      await useContextStore.getState().addProject('/b');
      expect(useContextStore.getState().projects).toEqual(['/a', '/b']);
    });
  });
});
