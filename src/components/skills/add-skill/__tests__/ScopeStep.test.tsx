/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ScopeStep } from '../ScopeStep';
import type { WizardState } from '../types';

const mocks = vi.hoisted(() => ({
  projectState: {
    projectsByEnvironment: {} as Record<string, unknown[]>,
  },
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

vi.mock('@/stores/projects', () => ({
  useProjectStore: (selector: (state: typeof mocks.projectState) => unknown) =>
    selector(mocks.projectState),
}));

function createState(): WizardState {
  return {
    step: 'scope',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    context: {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    },
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
    mocks.projectState.projectsByEnvironment = {};
  });

  it('uses the normalized shared directory path in the global option', () => {
    render(<ScopeStep state={createState()} updateState={vi.fn()} />);

    expect(screen.getByText('Global hint: ~/.agents/skills')).toBeDefined();
    expect(screen.queryByText('Global hint: ~/.agents/skills/')).toBeNull();
  });

  it('reads projects from the captured environment and updates the project ContextRef', () => {
    const state = createState();
    state.entryPoint = 'discovery';
    state.context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    mocks.projectState.projectsByEnvironment = { 'wsl:Ubuntu': [{
      binding: {
        id: 'project-1',
        nativePath: '/home/me/app',
        displayName: null,
        order: null,
        suppressCrossStorageWarning: false,
      },
      storage: {
        access: 'native',
        owner: { kind: 'wsl', distro_name: 'Ubuntu' },
      },
    }] };
    const updateState = vi.fn();

    render(<ScopeStep state={state} updateState={updateState} />);

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
