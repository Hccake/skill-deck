import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { UpdateExecutionRequest } from '@/bindings';

const { mockCommands } = vi.hoisted(() => ({
  mockCommands: {
    listAgents: vi.fn(),
    discoverSkillSource: vi.fn(),
    updateSkill: vi.fn(),
    openInstallWizard: vi.fn(),
  },
}));

vi.mock('@/bindings', () => ({
  commands: mockCommands,
}));

import {
  discoverSkillSource,
  listAgents,
  openInstallWizard,
  updateSkill,
} from '../useTauriApi';

const context = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  scope: { scope: 'project', project_id: 'project-1' },
} as const;

const previewToken = {
  generation: 'preview-1',
  registryRevision: 'registry-1',
  environmentRevision: 'environment-1',
  contextRevision: 'context-1',
};

describe('useTauriApi transport adapters', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('unwraps a successful generated Result', async () => {
    const snapshot = {
      registryRevision: 'registry-1',
      environmentRevision: 'environment-1',
      environment: context.environment,
      availability: 'available',
      projectPath: '/work/app',
      agents: {},
    };
    mockCommands.listAgents.mockResolvedValue({ status: 'ok', data: snapshot });

    await expect(listAgents(context)).resolves.toEqual(snapshot);
  });

  it('throws the unchanged error from a failed generated Result', async () => {
    const error = { kind: 'io', data: { message: 'file not found' } };
    mockCommands.listAgents.mockResolvedValue({ status: 'error', error });

    await expect(listAgents(context)).rejects.toEqual(error);
  });

  it('uses an empty source-selection intent when discovery does not provide one', async () => {
    const result = {
      discoverySession: {
        sessionId: 'discovery-1',
        environment: context.environment,
        sourceFingerprint: 'source-1',
        expiresAtEpochMs: 1000,
      },
      sourceType: 'git',
      sourceUrl: 'owner/repo',
      redirectedDownloadHost: null,
      gitRef: null,
      skillFilter: null,
      skills: [],
    };
    mockCommands.discoverSkillSource.mockResolvedValue({ status: 'ok', data: result });

    await expect(discoverSkillSource(
      context.environment,
      'owner/repo',
      'operation-1',
    )).resolves.toEqual(result);
    expect(mockCommands.discoverSkillSource).toHaveBeenCalledWith(
      context.environment,
      'owner/repo',
      'operation-1',
      { wildcardRequested: false, explicitSkillNames: [] },
    );
  });

  it('uses an unconfirmed redirect by default for update execution', async () => {
    const execution: UpdateExecutionRequest = {
      request: { context, skillNames: ['test-skill'] },
      overwritePrivateEntries: [],
    };
    const response = { sources: [], skills: [], outcome: 'succeeded' };
    mockCommands.updateSkill.mockResolvedValue({ status: 'ok', data: response });

    await expect(updateSkill(execution, previewToken)).resolves.toEqual(response);
    expect(mockCommands.updateSkill).toHaveBeenCalledWith(execution, previewToken, false);
  });

  it('maps optional wizard input to the generated positional contract', async () => {
    mockCommands.openInstallWizard.mockResolvedValue({ status: 'ok', data: null });

    await openInstallWizard({
      entryPoint: 'skills-panel',
      context,
      projectPath: '/work/app',
    });

    expect(mockCommands.openInstallWizard).toHaveBeenCalledWith(
      'skills-panel',
      context,
      '/work/app',
      null,
      null,
    );
  });
});
