/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstallPreviewOutcome } from '@/bindings';
import {
  acquireSelectedPayloads,
  checkSkillAudit,
  getInstallAgentSelection,
  previewInstall,
} from '@/hooks/useTauriApi';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import type { WizardState } from '../types';
import { ConfirmStep } from '../ConfirmStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  acquireSelectedPayloads: vi.fn(),
  previewInstall: vi.fn(),
  getInstallAgentSelection: vi.fn(),
  checkSkillAudit: vi.fn(),
}));

const acquirePayloads = vi.mocked(acquireSelectedPayloads);
const preview = vi.mocked(previewInstall);
const getSelection = vi.mocked(getInstallAgentSelection);
const audit = vi.mocked(checkSkillAudit);

function readyPreview(overwriteTargets: string[] = []): InstallPreviewOutcome {
  return {
    status: 'ready',
    preview: {
      token: {
        generation: 'preview-1',
        registryRevision: 'registry-1',
        environmentRevision: 'environment-1',
        contextRevision: 'context-1',
      },
      skills: [{ skillName: 'demo', overwriteTargets } as never],
    },
  };
}

function state(overrides: Partial<WizardState> = {}): WizardState {
  const agentSnapshot = {
    selection: makeAgentSelectionSnapshot({
      agents: [
        { kind: 'standard', id: 'codex', displayName: 'Codex', detection: 'detected', directoryAccess: 'standardOnly', installOptionId: null, groupId: null },
        { kind: 'standard', id: 'cursor', displayName: 'Cursor', detection: 'detected', directoryAccess: 'privateOnly', installOptionId: 'cursor-item', groupId: null },
      ],
      installOptions: [{ id: 'cursor-item', kind: 'standardDirectory', agentIds: ['cursor'], displayName: 'Cursor', path: '~/.cursor/skills', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null }],
      initialSelectedOptionIds: ['cursor-item'],
      userModeOptionIds: ['cursor-item'],
    }),
    defaultSelectionWarning: null,
  };
  return {
    step: 'confirm',
    entryPoint: 'skills-panel',
    scope: 'global',
    context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
    source: 'owner/repo',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    discoverySession: {
      sessionId: 'discovery-1',
      environment: { kind: 'native' },
      sourceFingerprint: 'source-1',
      expiresAtEpochMs: 1000,
    },
    riskPolicy: { kind: 'none', code: null },
    riskAcknowledged: false,
    availableSkills: [{ name: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md', pluginName: null, installDirName: 'demo' }],
    selectedSkills: ['demo'],
    skillFilter: null,
    skillSearchQuery: '',
    agentSelectionSnapshot: agentSnapshot,
    selectedAgentOptionIds: ['cursor-item'],
    expandedAgentGroupIds: [],
    additionalAgentsExpanded: false,
    selectionRequiresReconfirmation: false,
    mode: 'copy',
    otherAgentsExpanded: false,
    overwrites: {},
    preparation: { status: 'preparing' },
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    ...overrides,
  };
}

describe('ConfirmStep', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    acquirePayloads.mockResolvedValue([]);
    preview.mockResolvedValue(readyPreview());
    audit.mockResolvedValue(null);
  });

  it('previews the immutable selection submission from the Backend snapshot', async () => {
    const updateState = vi.fn();
    render(<ConfirmStep state={state()} updateState={updateState} scope="global" />);

    await waitFor(() => expect(preview).toHaveBeenCalledOnce());
    expect(preview).toHaveBeenCalledWith(expect.objectContaining({
      context: state().context,
      skills: ['demo'],
      agentSelection: {
        revision: 'selection-revision-1',
        selectedOptionIds: ['cursor-item'],
        requestedMode: 'copy',
      },
    }));
    expect(acquirePayloads).toHaveBeenCalledWith({
      discoverySession: state().discoverySession,
      skillPaths: ['skills/demo/SKILL.md'],
    });
  });

  it('returns to Agent selection with the latest snapshot when the revision is stale', async () => {
    const latest = state().agentSelectionSnapshot!;
    latest.selection.revision = 'selection-revision-2';
    latest.selection.installOptions.push({ id: 'new-item', kind: 'standardDirectory', agentIds: ['cursor'], displayName: 'Cursor extra', path: '~/.cursor/extra', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null });
    latest.selection.initialSelectedOptionIds = ['new-item'];
    preview.mockResolvedValue({ status: 'selectionStale', snapshot: latest });
    getSelection.mockResolvedValue(latest);
    const updateState = vi.fn();

    render(<ConfirmStep state={state()} updateState={updateState} scope="global" />);

    await waitFor(() => expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      step: 'options',
      agentSelectionSnapshot: latest,
      selectedAgentOptionIds: ['cursor-item', 'new-item'],
      selectionRequiresReconfirmation: true,
    })));
    expect(getSelection).toHaveBeenCalledWith(state().context, []);
  });

  it('publishes overwrite facts from the accepted preview', async () => {
    preview.mockResolvedValue(readyPreview(['/existing/demo']));
    const updateState = vi.fn();
    render(<ConfirmStep state={state()} updateState={updateState} scope="global" />);

    await waitFor(() => expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      overwrites: { demo: ['/existing/demo'] },
      preparation: expect.objectContaining({ status: 'ready' }),
    })));
  });

  it('shows the standard readers and selected placement from the snapshot', async () => {
    render(<ConfirmStep state={state()} updateState={vi.fn()} scope="global" />);

    await waitFor(() => expect(preview).toHaveBeenCalledOnce());
    expect(screen.getByText('Codex')).toBeDefined();
    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('~/.cursor/skills')).toBeDefined();
  });

  it('groups selected Agents by the link or copy action the user will see', async () => {
    const current = state({ mode: 'symlink' });
    current.agentSelectionSnapshot!.selection.agents.push({
      kind: 'standard', id: 'eve', displayName: 'Eve', detection: 'detected',
      directoryAccess: 'privateOnly', installOptionId: 'eve-item', groupId: null,
    });
    current.agentSelectionSnapshot!.selection.installOptions.push({
      id: 'eve-item', kind: 'standardDirectory', agentIds: ['eve'], displayName: 'Eve',
      path: './agent/skills', groupId: null, selectable: true,
      modeConstraint: 'copyOnly', disabledReason: null,
    });
    current.selectedAgentOptionIds = ['cursor-item', 'eve-item'];

    render(<ConfirmStep state={current} updateState={vi.fn()} scope="global" />);

    await waitFor(() => expect(preview).toHaveBeenCalledOnce());
    expect(screen.getByText('addSkill.confirm.createLinks')).toBeDefined();
    expect(screen.getByText('addSkill.confirm.createCopies')).toBeDefined();
    expect(screen.queryByText('agentSelection.title')).toBeNull();
  });

  it('keeps risk acknowledgement in the wizard state', async () => {
    const user = userEvent.setup();
    const updateState = vi.fn();
    render(
      <ConfirmStep
        state={state({ riskPolicy: { kind: 'require-confirmation', code: 'openclaw' } })}
        updateState={updateState}
        scope="global"
      />,
    );

    await user.click(screen.getByRole('checkbox'));
    expect(updateState).toHaveBeenCalledWith({ riskAcknowledged: true });
  });

  it('reports payload failures without attempting preview', async () => {
    acquirePayloads.mockRejectedValue({ kind: 'stalePayload' });
    const updateState = vi.fn();
    render(<ConfirmStep state={state()} updateState={updateState} scope="global" />);

    await waitFor(() => expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      preparation: expect.objectContaining({ status: 'failed', stage: 'payload' }),
    })));
    expect(preview).not.toHaveBeenCalled();
  });
});
