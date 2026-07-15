/* @vitest-environment jsdom */

import '@/test-utils';
import { MemoryRouter } from 'react-router-dom';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { canProceedForStep, getStepFlow } from '@/components/skills/add-skill/types';
import type { WizardState } from '@/components/skills/add-skill/types';
import { parseWizardContext } from '@/components/skills/add-skill/wizard-context';
import { useMutationStore } from '@/stores/mutation';
import { WizardPage } from '../WizardPage';

const mocks = vi.hoisted(() => ({
  requestAction: vi.fn().mockResolvedValue(undefined),
  emit: vi.fn().mockResolvedValue(undefined),
  refreshProjects: vi.fn().mockResolvedValue([]),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@tauri-apps/api/event', () => ({ emit: mocks.emit }));

vi.mock('@/hooks/useMutationMonitor', () => ({
  useMutationMonitor: vi.fn(),
}));

vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: mocks.requestAction }),
}));

vi.mock('@/stores/projects', () => ({
  useProjectStore: (selector: (state: { refresh: typeof mocks.refreshProjects }) => unknown) =>
    selector({ refresh: mocks.refreshProjects }),
}));

vi.mock('@/components/skills/add-skill/StepIndicator', () => ({
  StepIndicator: () => null,
}));

vi.mock('@/components/skills/add-skill/ScopeBadge', () => ({
  ScopeBadge: () => null,
}));

vi.mock('@/components/skills/add-skill/ScopeStep', () => ({
  ScopeStep: () => <div>scope-step</div>,
}));

vi.mock('@/components/skills/add-skill/SourceStep', () => ({
  SourceStep: ({ updateState }: {
    updateState: (updates: Partial<WizardState>) => void;
  }) => (
    <button
      type="button"
      onClick={() => updateState({
        source: 'openclaw/community-skills',
        fetchStatus: 'success',
        availableSkills: [{
          name: 'demo',
          installDirName: 'demo',
          description: 'Demo',
          relativePath: 'skills/demo/SKILL.md',
          pluginName: null,
        }],
        selectedSkills: ['demo'],
        selectedAgents: ['codex'],
        confirmReady: true,
      })}
    >
      prepare-source
    </button>
  ),
}));

vi.mock('@/components/skills/add-skill/SkillsStep', () => ({
  SkillsStep: () => <div>skills-step</div>,
}));

vi.mock('@/components/skills/add-skill/OptionsStep', () => ({
  OptionsStep: () => <div>options-step</div>,
}));

vi.mock('@/components/skills/add-skill/ConfirmStep', () => ({
  ConfirmStep: () => <div>confirm-step</div>,
}));

vi.mock('@/components/skills/add-skill/InstallingStep', () => ({
  InstallingStep: () => <div>installing-step</div>,
}));

vi.mock('@/components/skills/add-skill/CompleteStep', () => ({
  CompleteStep: () => <div>complete-step</div>,
}));

vi.mock('@/components/skills/add-skill/ErrorStep', () => ({
  ErrorStep: () => <div>error-step</div>,
}));

function createState(overrides: Partial<WizardState> = {}): WizardState {
  return {
    step: 'confirm',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    context: {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    },
    source: 'openclaw/community-skills',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    availableSkills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md', pluginName: null }],
    selectedSkills: ['demo'],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: ['codex'],
    allAgents: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: true,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    installError: undefined,
    retrySkillName: undefined,
    retryAgents: undefined,
    riskPolicy: { kind: 'require-confirmation', code: 'openclaw' },
    riskAcknowledged: false,
    ...overrides,
    privateCopyAgents: overrides.privateCopyAgents ?? [],
    privateCopyAgentsExpanded: overrides.privateCopyAgentsExpanded ?? false,
  };
}

describe('canProceedForStep', () => {
  it('blocks install on confirm step until guarded-source risk is acknowledged', () => {
    expect(canProceedForStep(createState())).toBe(false);
    expect(canProceedForStep(createState({ riskAcknowledged: true }))).toBe(true);
  });
});

describe('getStepFlow', () => {
  it('uses the selected context directly for Skills entry', () => {
    expect(getStepFlow('skills-panel')[0]).toBe('source');
  });

  it('keeps scope selection for Discover entry', () => {
    expect(getStepFlow('discovery')[0]).toBe('scope');
  });
});

describe('parseWizardContext', () => {
  it('restores the explicit WSL target from the wizard query', () => {
    expect(parseWizardContext(JSON.stringify({
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    }))).toEqual({
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    });
  });

  it('ignores malformed wizard context', () => {
    expect(parseWizardContext('{invalid')).toBeUndefined();
  });

  it('rejects structurally incomplete wizard context', () => {
    expect(parseWizardContext(JSON.stringify({
      environment: { kind: 'wsl' },
      scope: { scope: 'project' },
    }))).toBeUndefined();
  });
});

describe('WizardPage mutation guard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({
      activeMutation: null,
      loading: false,
      cancelling: false,
    });
  });

  it('loads projects for the environment frozen in the wizard URL', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };

    render(
      <MemoryRouter initialEntries={[
        `/wizard?entryPoint=discovery&context=${encodeURIComponent(JSON.stringify(context))}`,
      ]}>
        <WizardPage />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(mocks.refreshProjects).toHaveBeenCalledWith(context.environment);
    });
  });

  it('disables starting installation while another mutation is active', async () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: {
          environment: { kind: 'host' },
          scope: { scope: 'global' },
        },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    render(
      <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
        <WizardPage />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'prepare-source' }));
    await waitFor(() => {
      expect((screen.getByRole('button', { name: 'addSkill.actions.next' }) as HTMLButtonElement).disabled).toBe(false);
    });

    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));

    const install = await screen.findByRole('button', { name: 'addSkill.actions.install' });
    expect((install as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'addSkill.actions.back' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('routes the cancel action through the window lifecycle context', async () => {
    render(
      <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
        <WizardPage />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.cancel' }));

    await waitFor(() => {
      expect(mocks.requestAction).toHaveBeenCalledWith('closeCurrentWindow');
    });
  });
});
