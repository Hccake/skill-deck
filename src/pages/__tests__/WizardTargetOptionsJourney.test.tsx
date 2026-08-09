/* @vitest-environment jsdom */

import '@/test-utils';
import { MemoryRouter } from 'react-router-dom';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { WizardState } from '@/components/skills/add-skill/types';
import { TooltipProvider } from '@/components/ui/tooltip';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { useMutationStore } from '@/stores/mutation';
import { WizardPage } from '../WizardPage';

const mocks = vi.hoisted(() => ({ getSelection: vi.fn() }));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn() }));
vi.mock('@/hooks/useMutationMonitor', () => ({ useMutationMonitor: vi.fn() }));
vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: vi.fn() }),
}));
vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: () => ({ refresh: vi.fn().mockResolvedValue([]) }),
}));
vi.mock('@/hooks/useTauriApi', () => ({
  getInstallAgentSelection: (context: unknown, agents: unknown) => mocks.getSelection(context, agents),
}));
vi.mock('@/components/skills/add-skill/StepIndicator', () => ({ StepIndicator: () => null }));
vi.mock('@/components/skills/add-skill/ScopeBadge', () => ({ ScopeBadge: () => null }));
vi.mock('@/components/skills/add-skill/ScopeStep', () => ({ ScopeStep: () => null }));
vi.mock('@/components/skills/add-skill/SourceStep', () => ({
  SourceStep: ({ state, updateState }: {
    state: WizardState;
    updateState: (updates: Partial<WizardState>) => void;
  }) => (
    <button type="button" onClick={() => updateState({
      source: state.source === 'test/repo' ? 'other/repo' : 'test/repo',
      fetchStatus: 'success',
      availableSkills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md', pluginName: null }],
      selectedSkills: ['demo'],
      preSelectedAgents: state.source === 'test/repo' ? ['other-agent'] : [],
    })}>
      prepare-source
    </button>
  ),
}));
vi.mock('@/components/skills/add-skill/SkillsStep', () => ({ SkillsStep: () => <div>skills-step</div> }));
vi.mock('@/components/skills/add-skill/ConfirmStep', () => ({
  ConfirmStep: ({ agentSelection, updateState }: {
    agentSelection: { submission: { selectedOptionIds: string[] } };
    updateState: (updates: Partial<WizardState>) => void;
  }) => (
    <>
      <div>confirm-items:{agentSelection.submission.selectedOptionIds.join(',')}</div>
      <button type="button" onClick={() => updateState({
        preparation: {
          status: 'ready',
          prepared: { request: {} as never, preview: {} as never },
        },
      })}>
        prepare-install
      </button>
    </>
  ),
}));
vi.mock('@/components/skills/add-skill/InstallingStep', () => ({
  InstallingStep: ({ updateState }: {
    updateState: (updates: Partial<WizardState>) => void;
  }) => (
    <button type="button" onClick={() => updateState({
      installResults: {
        units: [{ status: 'failed', skillName: 'demo', retryable: true }],
        warnings: [],
      } as never,
      step: 'error',
    })}>
      fail-install
    </button>
  ),
}));
vi.mock('@/components/skills/add-skill/CompleteStep', () => ({
  CompleteStep: () => <div>install-result</div>,
}));
vi.mock('@/components/skills/add-skill/ErrorStep', () => ({ ErrorStep: () => null }));

describe('Wizard Agent selection journey', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    mocks.getSelection.mockResolvedValue({
      selection: makeAgentSelectionSnapshot({
        agents: [{ kind: 'standard', id: 'private-agent', displayName: 'Private Agent', detection: 'detected', directoryAccess: 'privateOnly', installOptionId: 'private-item', groupId: null }],
        installOptions: [{ id: 'private-item', kind: 'standardDirectory', agentIds: ['private-agent'], displayName: 'Private Agent', path: '~/.private-agent/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null }],
        userModeOptionIds: ['private-item'],
      }),
      defaultSelectionWarning: null,
    });
  });

  it('preserves a selected placement after review and reuses the loaded snapshot', async () => {
    render(
      <TooltipProvider>
        <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
          <WizardPage />
        </MemoryRouter>
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'prepare-source' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));

    const checkbox = await screen.findByRole('checkbox', { name: 'Private Agent' });
    fireEvent.click(checkbox);
    expect(checkbox.getAttribute('data-state')).toBe('checked');
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    expect(await screen.findByText('confirm-items:private-item')).toBeDefined();

    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.back' }));
    expect((await screen.findByRole('checkbox', { name: 'Private Agent' })).getAttribute('data-state')).toBe('checked');
    await waitFor(() => expect(mocks.getSelection).toHaveBeenCalledOnce());
  });

  it('preserves a selected placement after returning to the Skills step', async () => {
    render(
      <TooltipProvider>
        <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
          <WizardPage />
        </MemoryRouter>
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'prepare-source' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));

    const checkbox = await screen.findByRole('checkbox', { name: 'Private Agent' });
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.back' }));
    expect(await screen.findByText('skills-step')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));

    await waitFor(() => expect(mocks.getSelection).toHaveBeenCalledOnce());
    expect((await screen.findByRole('checkbox', { name: 'Private Agent' })).getAttribute('data-state'))
      .toBe('checked');
  });

  it('preserves a selected placement when retrying a failed installation', async () => {
    render(
      <TooltipProvider>
        <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
          <WizardPage />
        </MemoryRouter>
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'prepare-source' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Private Agent' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    expect(await screen.findByText('confirm-items:private-item')).toBeDefined();

    fireEvent.click(screen.getByRole('button', { name: 'prepare-install' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.install' }));
    fireEvent.click(await screen.findByRole('button', { name: 'fail-install' }));
    expect(await screen.findByText('install-result')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.retry' }));

    expect(await screen.findByText('confirm-items:private-item')).toBeDefined();
    await waitFor(() => expect(mocks.getSelection).toHaveBeenCalledOnce());
  });

  it('starts a new Agent selection session after the source changes its explicit Agents', async () => {
    mocks.getSelection.mockResolvedValueOnce({
      selection: makeAgentSelectionSnapshot({
        agents: [{ kind: 'standard', id: 'private-agent', displayName: 'Private Agent', detection: 'detected', directoryAccess: 'privateOnly', installOptionId: 'private-item', groupId: null }],
        installOptions: [{ id: 'private-item', kind: 'standardDirectory', agentIds: ['private-agent'], displayName: 'Private Agent', path: '~/.private-agent/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null }],
        userModeOptionIds: ['private-item'],
      }),
      defaultSelectionWarning: null,
    }).mockResolvedValueOnce({
      selection: makeAgentSelectionSnapshot({
        revision: 'selection-revision-2',
        agents: [{ kind: 'standard', id: 'other-agent', displayName: 'Other Agent', detection: 'detected', directoryAccess: 'privateOnly', installOptionId: 'other-item', groupId: null }],
        installOptions: [{ id: 'other-item', kind: 'standardDirectory', agentIds: ['other-agent'], displayName: 'Other Agent', path: '~/.other-agent/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null }],
        initialSelectedOptionIds: ['other-item'],
        userModeOptionIds: ['other-item'],
      }),
      defaultSelectionWarning: null,
    });
    render(
      <TooltipProvider>
        <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
          <WizardPage />
        </MemoryRouter>
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'prepare-source' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    expect(await screen.findByRole('checkbox', { name: 'Private Agent' })).toBeDefined();

    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.back' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.back' }));
    fireEvent.click(screen.getByRole('button', { name: 'prepare-source' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));

    expect(await screen.findByRole('checkbox', { name: 'Other Agent' })).toBeDefined();
    expect(screen.queryByRole('checkbox', { name: 'Private Agent' })).toBeNull();
    expect(mocks.getSelection).toHaveBeenNthCalledWith(2, expect.any(Object), ['other-agent']);
  });
});
