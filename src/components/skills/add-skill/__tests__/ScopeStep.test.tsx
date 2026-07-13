/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ScopeStep } from '../ScopeStep';
import type { WizardState } from '../types';

const mocks = vi.hoisted(() => ({
  listEnvironmentProjects: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === 'addSkill.scopeSelect.globalHint') {
        return `Global hint: ${String(options?.path ?? '')}`;
      }
      return key;
    },
  }),
}));

vi.mock('@/stores/context', () => ({
  useContextStore: (selector: (state: { projects: string[] }) => unknown) =>
    selector({ projects: ['D:/Code/hccake/skill-deck'] }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listEnvironmentProjects: (...args: unknown[]) => mocks.listEnvironmentProjects(...args),
}));

function createState(): WizardState {
  return {
    step: 'scope',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    source: '',
    fetchStatus: 'idle',
    fetchError: null,
    gitRef: null,
    riskPolicy: null,
    riskAcknowledged: false,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: [],
    privateCopyAgents: [],
    allAgents: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    privateCopyAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: false,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    installError: undefined,
    retrySkillName: undefined,
    retryAgents: undefined,
  };
}

describe('ScopeStep', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listEnvironmentProjects.mockResolvedValue([]);
  });

  it('uses the normalized shared directory path in the global option', () => {
    render(<ScopeStep state={createState()} updateState={vi.fn()} />);

    expect(screen.getByText('Global hint: ~/.agents/skills')).toBeDefined();
    expect(screen.queryByText('Global hint: ~/.agents/skills/')).toBeNull();
  });

  it('loads projects from the explicit environment and updates the project ContextRef', async () => {
    const state = createState();
    state.entryPoint = 'discovery';
    state.context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    mocks.listEnvironmentProjects.mockResolvedValue([{
      id: 'project-1',
      nativePath: '/home/me/app',
      displayName: null,
      order: null,
      suppressCrossStorageWarning: false,
    }]);
    const updateState = vi.fn();

    render(<ScopeStep state={state} updateState={updateState} />);

    await waitFor(() => expect(mocks.listEnvironmentProjects).toHaveBeenCalledWith(
      { kind: 'wsl', distro_name: 'Ubuntu' },
    ));
    fireEvent.click(screen.getByText('/home/me/app').closest('button')!);

    expect(updateState).toHaveBeenCalledWith({
      scope: 'project',
      projectPath: '/home/me/app',
      context: {
        environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        scope: { scope: 'project', project_id: 'project-1' },
      },
    });
  });
});
