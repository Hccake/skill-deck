/* @vitest-environment jsdom */

import '@/test-utils';
import { MemoryRouter } from 'react-router-dom';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { canProceedForStep, getStepFlow } from '@/components/skills/add-skill/types';
import type { WizardState } from '@/components/skills/add-skill/types';
import { parseWizardContext } from '@/components/skills/add-skill/wizard-context';
import { useMutationStore } from '@/stores/mutation';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { WizardPage } from '../WizardPage';

const mocks = vi.hoisted(() => ({
  requestAction: vi.fn().mockResolvedValue(undefined),
  emit: vi.fn().mockResolvedValue(undefined),
  refreshProjects: vi.fn().mockResolvedValue([]),
  getSelection: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@tauri-apps/api/event', () => ({ emit: mocks.emit }));

vi.mock('@/hooks/useMutationMonitor', () => ({
  useMutationMonitor: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getInstallAgentSelection: (...args: unknown[]) => mocks.getSelection(...args),
}));

vi.mock('@/hooks/useAgentSelectionSession', () => ({
  useAgentSelectionSession: () => ({
    status: 'ready',
    requiresReconfirmation: false,
  }),
}));

vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: mocks.requestAction }),
}));

vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: (environment: unknown) => ({
    refresh: () => mocks.refreshProjects(environment),
  }),
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
        preparation: {
          status: 'ready',
          prepared: { request: {} as never, preview: {} as never },
        },
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
  InstallingStep: ({ updateState }: {
    updateState: (updates: Partial<WizardState>) => void;
  }) => (
    <>
      <button
        type="button"
        onClick={() => updateState({
          installResults: {
            units: [{ status: 'succeeded', skillName: 'demo' }],
            warnings: [],
          } as never,
          step: 'complete',
        })}
      >
        finish-successful-install
      </button>
      <button
        type="button"
        onClick={() => updateState({
          installResults: {
            units: [
              { status: 'succeeded', skillName: 'demo' },
              { status: 'failed', skillName: 'broken', retryable: true },
            ],
            warnings: [],
          } as never,
          step: 'error',
        })}
      >
        finish-partial-install
      </button>
      <button
        type="button"
        onClick={() => updateState({
          installError: { message: 'install failed' },
          step: 'error',
        })}
      >
        finish-failed-install
      </button>
    </>
  ),
}));

vi.mock('@/components/skills/add-skill/CompleteStep', () => ({
  CompleteStep: () => <div data-testid="complete-step">complete-step</div>,
}));

vi.mock('@/components/skills/add-skill/ErrorStep', () => ({
  ErrorStep: () => <div data-testid="error-step">error-step</div>,
}));

function createState(overrides: Partial<WizardState> = {}): WizardState {
  return {
    step: 'confirm',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    context: {
      environment: { kind: 'native' },
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
    overwrites: {},
    preparation: {
      status: 'ready',
      prepared: { request: {} as never, preview: {} as never },
    },
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    installError: undefined,
    riskPolicy: { kind: 'require-confirmation', code: 'openclaw' },
    riskAcknowledged: false,
    ...overrides,
  };
}

function startInstallationFromSkillsEntry() {
  render(
    <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
      <WizardPage />
    </MemoryRouter>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'prepare-source' }));
  for (let step = 0; step < 3; step += 1) {
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
  }
  fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.install' }));
}

function expectFixedAction(button: HTMLElement) {
  expect(button.parentElement?.className).toContain('flex-shrink-0');
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
    mocks.getSelection.mockResolvedValue({
      selection: makeAgentSelectionSnapshot(),
      defaultSelectionWarning: null,
    });
    useMutationStore.setState({
      activeMutation: null,
      loading: false,
      cancelling: false,
    });
  });

  it('does not start a project refresh from the wizard page', () => {
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

    expect(mocks.refreshProjects).not.toHaveBeenCalled();
  });

  it('disables starting installation while another mutation is active', async () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: {
          environment: { kind: 'native' },
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

  it('notifies the main window as soon as an installation succeeds', async () => {
    startInstallationFromSkillsEntry();
    fireEvent.click(screen.getByRole('button', { name: 'finish-successful-install' }));

    await screen.findByTestId('complete-step');
    await waitFor(() => {
      expect(mocks.emit).toHaveBeenCalledWith('wizard-result', {
        action: 'refresh',
        context: {
          environment: { kind: 'native' },
          scope: { scope: 'global' },
        },
        mutatedSkillNames: ['demo'],
      });
    });
  });

  it('does not notify the main window again when the completed wizard closes', async () => {
    startInstallationFromSkillsEntry();
    fireEvent.click(screen.getByRole('button', { name: 'finish-successful-install' }));

    const done = await screen.findByRole('button', { name: 'addSkill.actions.done' });
    expectFixedAction(done);
    await waitFor(() => expect(mocks.emit).toHaveBeenCalledTimes(1));
    fireEvent.click(done);

    await waitFor(() => {
      expect(mocks.requestAction).toHaveBeenCalledWith('closeCurrentWindow');
    });
    expect(mocks.emit).toHaveBeenCalledTimes(1);
  });

  it('retries a failed main-window notification before closing the completed wizard', async () => {
    const errorLog = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    mocks.emit.mockRejectedValueOnce(new Error('event unavailable'));
    startInstallationFromSkillsEntry();
    fireEvent.click(screen.getByRole('button', { name: 'finish-successful-install' }));

    const done = await screen.findByRole('button', { name: 'addSkill.actions.done' });
    await waitFor(() => expect(mocks.emit).toHaveBeenCalledTimes(1));
    fireEvent.click(done);

    await waitFor(() => {
      expect(mocks.emit).toHaveBeenCalledTimes(2);
      expect(mocks.requestAction).toHaveBeenCalledWith('closeCurrentWindow');
    });
    errorLog.mockRestore();
  });

  it('notifies the main window about successful skills in a partial installation', async () => {
    startInstallationFromSkillsEntry();
    fireEvent.click(screen.getByRole('button', { name: 'finish-partial-install' }));

    await screen.findByTestId('complete-step');
    await waitFor(() => {
      expect(mocks.emit).toHaveBeenCalledWith('wizard-result', {
        action: 'refresh',
        context: {
          environment: { kind: 'native' },
          scope: { scope: 'global' },
        },
        mutatedSkillNames: ['demo'],
      });
    });
  });

  it('keeps result actions in the fixed footer while result content scrolls', async () => {
    startInstallationFromSkillsEntry();

    expect(screen.queryByRole('button', { name: 'addSkill.actions.done' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'finish-partial-install' }));

    const retry = await screen.findByRole('button', { name: 'addSkill.actions.retry' });
    const done = screen.getByRole('button', { name: 'addSkill.actions.done' });
    expectFixedAction(retry);
    expectFixedAction(done);
    expect(screen.getByTestId('complete-step').contains(retry)).toBe(false);

    fireEvent.click(retry);
    expect(await screen.findByText('confirm-step')).toBeDefined();
  });

  it('keeps fatal error actions in the fixed footer', async () => {
    startInstallationFromSkillsEntry();
    fireEvent.click(screen.getByRole('button', { name: 'finish-failed-install' }));

    await screen.findByTestId('error-step');
    const close = screen.getByRole('button', { name: 'addSkill.error.actions.close' });
    const back = screen.getByRole('button', { name: 'addSkill.error.actions.backToSource' });
    const retry = screen.getByRole('button', { name: 'addSkill.error.actions.retry' });
    expectFixedAction(close);
    expectFixedAction(back);
    expectFixedAction(retry);
  });
});
