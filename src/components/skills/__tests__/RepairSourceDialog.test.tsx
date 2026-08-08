/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { RepairSourceDialog } from '../RepairSourceDialog';
import type { InstalledSkill } from '@/bindings';

const mocks = vi.hoisted(() => ({
  fetchAvailable: vi.fn(),
  acquireSelectedPayloads: vi.fn(),
  previewInstall: vi.fn(),
  installSkills: vi.fn(),
  getInstallAgentSelection: vi.fn(),
  markSourceRepairSucceeded: vi.fn(),
  syncSkills: vi.fn(),
  toastSuccess: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.name ? `${key}:${options.name}` : key,
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  fetchAvailable: (...args: unknown[]) => mocks.fetchAvailable(...args),
  acquireSelectedPayloads: (...args: unknown[]) => mocks.acquireSelectedPayloads(...args),
  previewInstall: (...args: unknown[]) => mocks.previewInstall(...args),
  installSkills: (...args: unknown[]) => mocks.installSkills(...args),
  getInstallAgentSelection: (...args: unknown[]) => mocks.getInstallAgentSelection(...args),
}));

vi.mock('@/components/recovery/RecoveryActions', () => ({
  RecoveryActions: ({ recovery, onResolved }: {
    recovery: { resourceId: string };
    onResolved?: () => void;
  }) => (
    <button type="button" onClick={onResolved}>recovery-actions:{recovery.resourceId}</button>
  ),
}));

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: mocks.toastSuccess },
}));
vi.mock('@/utils/cross-storage-guidance', () => ({
  appendCrossStorageFailureGuidance: (message: string) => message,
}));
vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: unknown) => unknown) => selector({
    markSourceRepairSucceeded: mocks.markSourceRepairSucceeded,
    syncSkills: mocks.syncSkills,
  }),
}));

const context = {
  environment: { kind: 'native' },
  scope: { scope: 'global' },
} as const;
const discoverySession = {
  sessionId: 'discovery-1',
  environment: context.environment,
  sourceFingerprint: 'source-1',
  expiresAtEpochMs: 1000,
};
const token = {
  generation: 'preview-1',
  registryRevision: 'registry-1',
  environmentRevision: 'environment-1',
  contextRevision: 'context-1',
};

function openDialog(source = 'owner/repo') {
  useSkillDialogStore.setState({
    repairSourceTarget: {
      skillName: 'toolkit',
      source,
      scope: 'global',
      context,
      agents: ['claude-code'],
      gitRef: null,
    },
  });
  render(<RepairSourceDialog />);
}

describe('RepairSourceDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
    useSkillDialogStore.setState({
      copySkill: null,
      copyContext: undefined,
      repairSourceTarget: null,
    });
    mocks.fetchAvailable.mockResolvedValue({
      discoverySession,
      sourceType: 'github',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: null,
      skillFilter: null,
      riskPolicy: { kind: 'none', code: null },
      skills: [{
        name: 'toolkit',
        installDirName: 'toolkit',
        description: 'Toolkit',
        relativePath: 'skills/toolkit',
      }],
    });
    mocks.acquireSelectedPayloads.mockResolvedValue([]);
    mocks.getInstallAgentSelection.mockResolvedValue({
      selection: {
        agents: [], installOptions: [], groups: [], initialSelectedOptionIds: ['claude-item'],
        unavailableExplicitAgents: [], userModeOptionIds: ['claude-item'], revision: 'selection-1',
      },
      defaultSelectionWarning: null,
    });
    mocks.previewInstall.mockResolvedValue({ status: 'ready', preview: { token, skills: [] } });
    mocks.installSkills.mockResolvedValue({
      units: [{ unitId: 'toolkit', status: 'succeeded' }],
    });
    mocks.syncSkills.mockResolvedValue(undefined);
  });

  it('executes source repair through discovery, payload, preview, and execute', async () => {
    openDialog();
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => expect(mocks.installSkills).toHaveBeenCalled());
    expect(mocks.acquireSelectedPayloads).toHaveBeenCalledWith({
      discoverySession,
      skillPaths: ['skills/toolkit'],
    });
    const request = mocks.previewInstall.mock.calls[0][0];
    expect(request).toMatchObject({
      context,
      source: 'owner/repo',
      skills: ['toolkit'],
      agentSelection: {
        revision: 'selection-1',
        selectedOptionIds: ['claude-item'],
        requestedMode: 'copy',
      },
    });
    expect(mocks.installSkills).toHaveBeenCalledWith(request, token);
    expect(mocks.markSourceRepairSucceeded).toHaveBeenCalledWith(context, 'toolkit');
    expect(mocks.toastSuccess).not.toHaveBeenCalled();
  });

  it('opens source repair with an empty input when the source record is missing', () => {
    useSkillDialogStore.getState().openRepairSource({
      name: 'toolkit',
      description: '',
      path: '/skills/toolkit',
      canonicalPath: '/skills/toolkit',
      scope: 'global',
      agents: ['claude-code'],
      associatedAgents: ['claude-code'],
      source: null,
      sourceUrl: null,
    } as InstalledSkill, context);

    render(<RepairSourceDialog />);

    expect((screen.getByRole('textbox', {
      name: 'skills.repairSourceDialog.sourceLabel',
    }) as HTMLInputElement).value).toBe('');
  });

  it('prompts the matching Copy session to retry after repair succeeds', async () => {
    const copySkill = {
      name: 'toolkit',
      description: '',
      path: '/skills/toolkit',
      canonicalPath: '/skills/toolkit',
      scope: 'global',
      agents: ['claude-code'],
      associatedAgents: ['claude-code'],
    } as InstalledSkill;
    useSkillDialogStore.setState({ copySkill, copyContext: context });
    openDialog();

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => expect(useSkillDialogStore.getState().repairSourceTarget).toBeNull());
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      'skills.repairSourceDialog.copyRetry',
    );
    expect(useSkillDialogStore.getState().copySkill).toBe(copySkill);
  });

  it('keeps the dialog open when a mutation unit fails', async () => {
    mocks.installSkills.mockResolvedValue({
      units: [{ unitId: 'toolkit', status: 'failed' }],
    });
    openDialog();
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => expect(mocks.installSkills).toHaveBeenCalled());
    expect(mocks.markSourceRepairSucceeded).not.toHaveBeenCalled();
    expect(useSkillDialogStore.getState().repairSourceTarget).not.toBeNull();
    expect(screen.getByRole('alert').textContent)
      .toContain('skills.repairSourceDialog.repairFailed');
    expect(mocks.toastSuccess).not.toHaveBeenCalled();
  });

  it('requires a fresh dialog after recovery is resolved', async () => {
    mocks.installSkills.mockResolvedValue({
      units: [{
        unitId: 'toolkit',
        status: 'recoveryRequired',
        recovery: { resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' },
      }],
    });
    openDialog();
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await screen.findByRole('button', { name: 'recovery-actions:recovery-1' });
    expect(screen.queryByRole('button', { name: 'skills.repairSourceDialog.repair' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'skills.repairSourceDialog.validate' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'recovery-actions:recovery-1' }));

    expect(useSkillDialogStore.getState().repairSourceTarget).toBeNull();
  });

  it('prevents dismissal during repair and exposes an explicit stop action', async () => {
    mocks.installSkills.mockImplementation(() => new Promise(() => undefined));
    const cancelActiveMutation = vi.fn().mockResolvedValue(true);
    useMutationStore.setState({ cancelActiveMutation });
    openDialog();

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));
    await waitFor(() => expect(mocks.installSkills).toHaveBeenCalled());

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(useSkillDialogStore.getState().repairSourceTarget).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.stop' }));
    expect(cancelActiveMutation).toHaveBeenCalledTimes(1);
  });

  it('requires explicit acknowledgement for guarded sources', async () => {
    mocks.fetchAvailable.mockResolvedValue({
      ...(await mocks.fetchAvailable()),
      riskPolicy: { kind: 'require-confirmation', code: 'guarded' },
    });
    openDialog('guarded/repo');
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await screen.findByRole('checkbox');
    expect(mocks.installSkills).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));
    await waitFor(() => expect(mocks.installSkills).toHaveBeenCalled());
  });

  it('blocks repair while another mutation is active', () => {
    useMutationStore.setState({
      activeMutation: {
        id: 'mutation-1',
        kind: 'update',
        context,
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });
    openDialog();
    expect((screen.getByRole('button', {
      name: 'skills.repairSourceDialog.repair',
    }) as HTMLButtonElement).disabled).toBe(true);
  });
});
