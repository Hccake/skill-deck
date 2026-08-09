/* @vitest-environment jsdom */

import '@/test-utils';
import { useEffect } from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstallAgentSelectionSnapshot, InstallMode, InstallPreviewOutcome } from '@/bindings';
import { useAgentSelectionSession } from '@/hooks/useAgentSelectionSession';
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

function agentSnapshot(): InstallAgentSelectionSnapshot {
  return {
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
}

function state(overrides: Partial<WizardState> = {}): WizardState {
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
    overwrites: {},
    preparation: { status: 'preparing' },
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    ...overrides,
  };
}

function ConfirmHarness({
  current,
  selection = agentSnapshot(),
  requestedMode = 'copy',
  updateState,
}: {
  current: WizardState;
  selection?: InstallAgentSelectionSnapshot;
  requestedMode?: InstallMode;
  updateState: (updates: Partial<WizardState>) => void;
}) {
  const agentSelection = useAgentSelectionSession({
    active: true,
    request: {
      kind: 'install',
      context: current.context,
      explicitAgentIds: current.preSelectedAgents,
    },
    load: async () => selection,
  });
  const currentMode = agentSelection.status === 'ready' ? agentSelection.session.mode : null;
  const setMode = agentSelection.setMode;
  useEffect(() => {
    if (currentMode !== null && currentMode !== requestedMode) {
      setMode(requestedMode);
    }
  }, [currentMode, requestedMode, setMode]);
  if (agentSelection.status !== 'ready' || currentMode !== requestedMode) return null;
  return (
    <ConfirmStep
      state={current}
      agentSelection={agentSelection}
      updateState={updateState}
      scope="global"
    />
  );
}

function renderConfirm(
  current = state(),
  updateState = vi.fn(),
  selection = agentSnapshot(),
  requestedMode: InstallMode = 'copy',
) {
  render(
    <ConfirmHarness
      current={current}
      selection={selection}
      requestedMode={requestedMode}
      updateState={updateState}
    />,
  );
  return { updateState };
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
    renderConfirm(state(), updateState);

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
    const latest = agentSnapshot();
    latest.selection.revision = 'selection-revision-2';
    latest.selection.installOptions.push({ id: 'new-item', kind: 'standardDirectory', agentIds: ['cursor'], displayName: 'Cursor extra', path: '~/.cursor/extra', groupId: null, selectable: true, modeConstraint: 'userSelectable', disabledReason: null });
    latest.selection.initialSelectedOptionIds = ['new-item'];
    preview.mockResolvedValue({ status: 'selectionStale', snapshot: latest });
    getSelection.mockResolvedValue(latest);
    const updateState = vi.fn();

    renderConfirm(state(), updateState);

    await waitFor(() => expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      step: 'options',
      preparation: { status: 'idle' },
    })));
    expect(getSelection).toHaveBeenCalledWith(state().context, []);
  });

  it('publishes overwrite facts from the accepted preview', async () => {
    preview.mockResolvedValue(readyPreview(['/existing/demo']));
    const updateState = vi.fn();
    renderConfirm(state(), updateState);

    await waitFor(() => expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      overwrites: { demo: ['/existing/demo'] },
      preparation: expect.objectContaining({ status: 'ready' }),
    })));
  });

  it('shows the standard readers and selected placement from the snapshot', async () => {
    renderConfirm();

    await waitFor(() => expect(preview).toHaveBeenCalledOnce());
    expect(screen.getByText('Codex')).toBeDefined();
    expect(screen.getByText('Cursor')).toBeDefined();
    expect(screen.getByText('~/.cursor/skills')).toBeDefined();
  });

  it('groups selected Agents by the link or copy action the user will see', async () => {
    const current = state();
    const selection = agentSnapshot();
    selection.selection.agents.push({
      kind: 'standard', id: 'eve', displayName: 'Eve', detection: 'detected',
      directoryAccess: 'privateOnly', installOptionId: 'eve-item', groupId: null,
    });
    selection.selection.installOptions.push({
      id: 'eve-item', kind: 'standardDirectory', agentIds: ['eve'], displayName: 'Eve',
      path: './agent/skills', groupId: null, selectable: true,
      modeConstraint: 'copyOnly', disabledReason: null,
    });
    selection.selection.initialSelectedOptionIds = ['cursor-item', 'eve-item'];

    renderConfirm(current, vi.fn(), selection, 'symlink');

    await waitFor(() => expect(preview).toHaveBeenCalledOnce());
    expect(screen.getByText('addSkill.confirm.createLinks')).toBeDefined();
    expect(screen.getByText('addSkill.confirm.createCopies')).toBeDefined();
    expect(screen.queryByText('agentSelection.title')).toBeNull();
  });

  it('keeps risk acknowledgement in the wizard state', async () => {
    const user = userEvent.setup();
    const updateState = vi.fn();
    renderConfirm(
      state({ riskPolicy: { kind: 'require-confirmation', code: 'openclaw' } }),
      updateState,
    );

    await user.click(await screen.findByRole('checkbox'));
    expect(updateState).toHaveBeenCalledWith({ riskAcknowledged: true });
  });

  it('reports payload failures without attempting preview', async () => {
    acquirePayloads.mockRejectedValue({ kind: 'stalePayload' });
    const updateState = vi.fn();
    renderConfirm(state(), updateState);

    await waitFor(() => expect(updateState).toHaveBeenCalledWith(expect.objectContaining({
      preparation: expect.objectContaining({ status: 'failed', stage: 'payload' }),
    })));
    expect(preview).not.toHaveBeenCalled();
  });
});
