import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { createAgentSelectionSession } from '@/lib/agent-selection-session';
import type { AgentSelectionSessionController } from '@/hooks/useAgentSelectionSession';
import { AgentSelectionPanel } from '../AgentSelectionPanel';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

describe('AgentSelectionPanel', () => {
  it('distinguishes current and compatible directories without losing an existing selection', () => {
    const selection = makeAgentSelectionSnapshot({
      agents: [{ kind: 'standard', id: 'kilo', displayName: 'Kilo Code', detection: 'detected',
        directoryAccess: 'both', installOptionId: 'current', groupId: null }],
      installOptions: ['current', 'compatible'].map((id) => ({ id, kind: 'standardDirectory', agentIds: ['kilo'],
        displayName: 'Kilo Code', path: id === 'current' ? '/project/.kilo/skills' : '/project/.kilocode/skills',
        groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null })),
      baselineSelectedOptionIds: ['compatible'], userModeOptionIds: ['current', 'compatible'],
    });
    const setOptionSelected = vi.fn();
    const controller: AgentSelectionSessionController<{ selection: typeof selection }> = {
      status: 'ready', snapshot: { selection }, selection, session: createAgentSelectionSession(selection), optionStates: [],
      submission: { revision: selection.revision, selectedOptionIds: ['compatible'], requestedMode: 'copy' },
      requiresReconfirmation: false, retry: vi.fn(), setOptionSelected, setMode: vi.fn(), setGroupSelected: vi.fn(),
      setOtherAgentsExpanded: vi.fn(), setAdditionalInstallExpanded: vi.fn(), setGroupExpanded: vi.fn(),
      acceptSnapshot: vi.fn(), confirmCurrentSelection: vi.fn(), isDirty: false,
    };
    render(<TooltipProvider><AgentSelectionPanel usage="install" controller={controller} /></TooltipProvider>);
    expect(screen.getByRole('checkbox', { name: /agentSelection.compatibleDirectory/ }).getAttribute('aria-checked')).toBe('true');
    fireEvent.click(screen.getByRole('checkbox', { name: /agentSelection.currentDirectory/ }));
    expect(setOptionSelected).toHaveBeenCalledWith('current', true);
  });

  it('connects the existing selection view to the session controller', () => {
    const selection = makeAgentSelectionSnapshot({
      agents: [{
        kind: 'standard',
        id: 'claude',
        displayName: 'Claude',
        detection: 'detected',
        directoryAccess: 'privateOnly',
        installOptionId: 'claude',
        groupId: null,
      }],
      installOptions: [{
        id: 'claude',
        kind: 'standardDirectory',
        agentIds: ['claude'],
        displayName: 'Claude',
        path: '/agents/claude',
        groupId: null,
        selectable: true,
        modeConstraint: 'userSelectable',
        disabledReason: null,
      }],
      baselineSelectedOptionIds: ['claude'],
      userModeOptionIds: ['claude'],
    });
    const setOptionSelected = vi.fn();
    const controller: AgentSelectionSessionController<{ selection: typeof selection }> = {
      status: 'ready',
      snapshot: { selection },
      selection,
      session: createAgentSelectionSession(selection),
      optionStates: [],
      submission: {
        revision: selection.revision,
        selectedOptionIds: ['claude'],
        requestedMode: 'symlink',
      },
      requiresReconfirmation: false,
      retry: vi.fn(),
      setOptionSelected,
      setMode: vi.fn(),
      setGroupSelected: vi.fn(),
      setOtherAgentsExpanded: vi.fn(),
      setAdditionalInstallExpanded: vi.fn(),
      setGroupExpanded: vi.fn(),
      acceptSnapshot: vi.fn(),
      confirmCurrentSelection: vi.fn(),
      isDirty: false,
    };

    render(
      <TooltipProvider>
        <AgentSelectionPanel
          usage="install"
          controller={controller}
          emptyMessage="No Agents"
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole('checkbox', { name: 'Claude' }));
    expect(setOptionSelected).toHaveBeenCalledWith('claude', false);
  });
});
