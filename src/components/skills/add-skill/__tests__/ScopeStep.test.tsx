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
      if (key === 'context.environmentWslName') {
        return `WSL · ${String(options?.environment ?? '')}`;
      }
      return key;
    },
  }),
}));

vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: (environment: { kind: string; distro_name?: string }) => {
    const key = environment.kind === 'native' ? 'native' : `wsl:${environment.distro_name?.toLowerCase()}`;
    return { projects: mocks.projectState.projectsByEnvironment[key] ?? [] };
  },
}));

function createState(): WizardState {
  return {
    step: 'scope',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    context: {
      environment: { kind: 'native' },
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
    overwrites: {},
    preparation: { status: 'idle' },
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    installError: undefined,
  };
}

describe('ScopeStep', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.projectState.projectsByEnvironment = {};
  });

  it('uses the normalized standard directory path in the global option', () => {
    render(<ScopeStep state={createState()} updateState={vi.fn()} />);

    expect(screen.getByRole('radiogroup', {
      name: 'addSkill.scopeSelect.title',
    })).toBeDefined();
    expect(screen.getByRole('radio', {
      name: /addSkill.scopeSelect.global/,
    }).getAttribute('aria-checked')).toBe('true');
    expect(screen.getByText('Global hint: ~/.agents/skills')).toBeDefined();
    expect(screen.queryByText('Global hint: ~/.agents/skills/')).toBeNull();
  });

  it('reads projects from the captured environment and updates the project SkillLocationRef', () => {
    const state = createState();
    state.entryPoint = 'discovery';
    state.context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    mocks.projectState.projectsByEnvironment = { 'wsl:ubuntu': [{
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

    expect(screen.getByText('WSL · Ubuntu')).toBeDefined();

    fireEvent.click(screen.getByRole('radio', { name: /\/home\/me\/app/ }));

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
