/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { RepairSourceDialog } from '../RepairSourceDialog';

const mocks = vi.hoisted(() => ({
  fetchAvailable: vi.fn(),
  acquireSelectedPayloads: vi.fn(),
  previewInstall: vi.fn(),
  installSkills: vi.fn(),
  markSourceRepairSucceeded: vi.fn(),
  syncSkills: vi.fn(),
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
}));

vi.mock('sonner', () => ({ toast: { error: vi.fn() } }));
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
  environment: { kind: 'host' },
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
    useSkillDialogStore.setState({ repairSourceTarget: null });
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
    mocks.previewInstall.mockResolvedValue({ token, skills: [] });
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
      agentIntents: [{
        agentId: 'claude-code',
        privateEntry: 'required',
        adapterTargets: [],
      }],
      requestedMode: 'copy',
    });
    expect(mocks.installSkills).toHaveBeenCalledWith(request, token);
    expect(mocks.markSourceRepairSucceeded).toHaveBeenCalledWith(context, 'toolkit');
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
