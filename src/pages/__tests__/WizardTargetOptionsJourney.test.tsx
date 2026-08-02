/* @vitest-environment jsdom */

import '@/test-utils';
import { MemoryRouter } from 'react-router-dom';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { makeAgentRuntimeSnapshot, makeResolvedAgent, makeResolvedAgentScope } from '@/test-utils';
import type { WizardState } from '@/components/skills/add-skill/types';
import { useMutationStore } from '@/stores/mutation';
import { WizardPage } from '../WizardPage';

const mocks = vi.hoisted(() => ({
  listAgents: vi.fn(),
  listGroups: vi.fn(),
  listTargets: vi.fn(),
  getDefaults: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn() }));
vi.mock('@/hooks/useMutationMonitor', () => ({ useMutationMonitor: vi.fn() }));
vi.mock('@/lifecycle/useWindowLifecycle', () => ({
  useWindowLifecycle: () => ({ requestAction: vi.fn() }),
}));
vi.mock('@/stores/projects', () => ({
  useProjectStore: (selector: (state: { refresh: () => Promise<never[]> }) => unknown) => (
    selector({ refresh: vi.fn().mockResolvedValue([]) })
  ),
}));
vi.mock('@/hooks/useTauriApi', () => ({
  listAgents: (context: unknown) => mocks.listAgents(context),
  listAgentSelectionGroups: (context: unknown) => mocks.listGroups(context),
  listEveInstallTargets: (context: unknown) => mocks.listTargets(context),
  getDefaultTargetAgents: (context: unknown) => mocks.getDefaults(context),
}));
vi.mock('@/components/skills/add-skill/StepIndicator', () => ({ StepIndicator: () => null }));
vi.mock('@/components/skills/add-skill/ScopeBadge', () => ({ ScopeBadge: () => null }));
vi.mock('@/components/skills/add-skill/ScopeStep', () => ({ ScopeStep: () => null }));
vi.mock('@/components/skills/add-skill/SourceStep', () => ({
  SourceStep: ({ updateState }: {
    updateState: (updates: Partial<WizardState>) => void;
  }) => (
    <button
      type="button"
      onClick={() => updateState({
        source: 'test/repo',
        fetchStatus: 'success',
        availableSkills: [{
          name: 'demo',
          installDirName: 'demo',
          description: 'Demo',
          relativePath: 'skills/demo/SKILL.md',
          pluginName: null,
        }],
        selectedSkills: ['demo'],
        preSelectedAgents: [],
      })}
    >
      prepare-source
    </button>
  ),
}));
vi.mock('@/components/skills/add-skill/SkillsStep', () => ({
  SkillsStep: () => <div>skills-step</div>,
}));
vi.mock('@/components/skills/add-skill/ConfirmStep', () => ({
  ConfirmStep: ({ state }: { state: WizardState }) => (
    <div>confirm-targets:{state.selectedAgents.join(',')}</div>
  ),
}));
vi.mock('@/components/skills/add-skill/InstallingStep', () => ({ InstallingStep: () => null }));
vi.mock('@/components/skills/add-skill/CompleteStep', () => ({ CompleteStep: () => null }));
vi.mock('@/components/skills/add-skill/ErrorStep', () => ({ ErrorStep: () => null }));

describe('Wizard target options journey', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    const agent = makeResolvedAgent({
      id: 'private-agent',
      displayName: 'Private Agent',
      global: makeResolvedAgentScope({
        readsShared: false,
        privatePath: '~/.private-agent/skills',
        readPaths: ['~/.private-agent/skills'],
      }),
    });
    mocks.listAgents.mockResolvedValue(makeAgentRuntimeSnapshot([agent]));
    mocks.listGroups.mockResolvedValue({ global: [], project: [] });
    mocks.listTargets.mockResolvedValue([]);
    mocks.getDefaults.mockResolvedValue({ global: [], project: [] });
  });

  it('preserves an explicit separate Agent after review and reuses the loaded facts', async () => {
    render(
      <MemoryRouter initialEntries={['/wizard?entryPoint=skills-panel']}>
        <WizardPage />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'prepare-source' }));
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    expect(screen.getByText('skills-step')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));

    const checkbox = await screen.findByRole('checkbox', { name: /Private Agent/ });
    fireEvent.click(checkbox);
    expect(checkbox.getAttribute('data-state')).toBe('checked');
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.next' }));
    expect(await screen.findByText('confirm-targets:private-agent')).toBeDefined();

    fireEvent.click(screen.getByRole('button', { name: 'addSkill.actions.back' }));
    const restoredCheckbox = await screen.findByRole('checkbox', { name: /Private Agent/ });
    expect(restoredCheckbox.getAttribute('data-state')).toBe('checked');
    await waitFor(() => expect(mocks.listAgents).toHaveBeenCalledTimes(1));
    expect(mocks.listGroups).toHaveBeenCalledTimes(1);
    expect(mocks.getDefaults).toHaveBeenCalledTimes(1);
  });
});
