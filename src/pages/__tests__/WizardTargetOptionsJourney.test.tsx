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
vi.mock('@/stores/projects', () => ({
  useProjectStore: (selector: (state: { refresh: () => Promise<never[]> }) => unknown) => (
    selector({ refresh: vi.fn().mockResolvedValue([]) })
  ),
}));
vi.mock('@/hooks/useTauriApi', () => ({
  getInstallAgentSelection: (context: unknown, agents: unknown) => mocks.getSelection(context, agents),
}));
vi.mock('@/components/skills/add-skill/StepIndicator', () => ({ StepIndicator: () => null }));
vi.mock('@/components/skills/add-skill/ScopeBadge', () => ({ ScopeBadge: () => null }));
vi.mock('@/components/skills/add-skill/ScopeStep', () => ({ ScopeStep: () => null }));
vi.mock('@/components/skills/add-skill/SourceStep', () => ({
  SourceStep: ({ updateState }: { updateState: (updates: Partial<WizardState>) => void }) => (
    <button type="button" onClick={() => updateState({
      source: 'test/repo',
      fetchStatus: 'success',
      availableSkills: [{ name: 'demo', installDirName: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md', pluginName: null }],
      selectedSkills: ['demo'],
    })}>
      prepare-source
    </button>
  ),
}));
vi.mock('@/components/skills/add-skill/SkillsStep', () => ({ SkillsStep: () => <div>skills-step</div> }));
vi.mock('@/components/skills/add-skill/ConfirmStep', () => ({
  ConfirmStep: ({ state }: { state: WizardState }) => (
    <div>confirm-items:{state.selectedAgentItemIds.join(',')}</div>
  ),
}));
vi.mock('@/components/skills/add-skill/InstallingStep', () => ({ InstallingStep: () => null }));
vi.mock('@/components/skills/add-skill/CompleteStep', () => ({ CompleteStep: () => null }));
vi.mock('@/components/skills/add-skill/ErrorStep', () => ({ ErrorStep: () => null }));

describe('Wizard Agent selection journey', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false, cancelling: false });
    mocks.getSelection.mockResolvedValue({
      selection: makeAgentSelectionSnapshot({
        agents: [{ id: 'private-agent', displayName: 'Private Agent', detection: 'detected' }],
        items: [{ id: 'private-item', agentIds: ['private-agent'], category: 'separateInstall', displayName: 'Private Agent', path: '~/.private-agent/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null }],
        requestedModeItemIds: ['private-item'],
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
});
