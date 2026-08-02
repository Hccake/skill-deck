// src/hooks/__tests__/useTauriApi.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { CustomAgentDefinition, InstallRequest } from '@/bindings';

const { mockCommands } = vi.hoisted(() => ({
  mockCommands: {
    listAgents: vi.fn(),
    listSkills: vi.fn(),
    listEveInstallTargets: vi.fn(),
    readSkillContent: vi.fn(),
    previewInstall: vi.fn(),
    installSkills: vi.fn(),
    previewUpdate: vi.fn(),
    updateSkill: vi.fn(),
    removeSkill: vi.fn(),
    getSkillAgentDetails: vi.fn(),
    manageSkillAgents: vi.fn(),
    cleanupDuplicateAgentCopies: vi.fn(),
    copySkillToProjects: vi.fn(),
    saveDefaultTargetAgents: vi.fn(),
    checkOverwrites: vi.fn(),
    checkUpdates: vi.fn(),
    updateSkillsBatch: vi.fn(),
    mapEnvironmentPath: vi.fn(),
    setEnvironmentProjectCrossStorageWarning: vi.fn(),
    openInstallWizard: vi.fn(),
    getInstallWizardSession: vi.fn(),
    focusInstallWizard: vi.fn(),
    getConfig: vi.fn(),
    getAgentSettingsSnapshot: vi.fn(),
    validateCustomAgentDraft: vi.fn(),
    saveCustomAgent: vi.fn(),
    previewCustomAgentDelete: vi.fn(),
    deleteCustomAgent: vi.fn(),
    deleteInvalidCustomAgent: vi.fn(),
    getRecoveryResourceStatus: vi.fn(),
    confirmRecoveryResourceResolved: vi.fn(),
    openRecoveryResource: vi.fn(),
  },
}));

vi.mock('@/bindings', () => ({
  commands: mockCommands,
}));

import {
  installSkills,
  previewInstall,
  listAgents,
  listEveInstallTargets,
  listSkills,
  mapEnvironmentPath,
  openInstallWizard,
  getInstallWizardSession,
  focusInstallWizard,
  readSkillContent,
  saveDefaultTargetAgents,
  setEnvironmentProjectCrossStorageWarning,
  updateSkill,
  getAgentSettingsSnapshot,
  validateCustomAgentDraft,
  saveCustomAgent,
  previewCustomAgentDelete,
  deleteCustomAgent,
  deleteInvalidCustomAgent,
  getRecoveryResourceStatus,
  confirmRecoveryResourceResolved,
  openRecoveryResource,
  checkUpdates,
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

describe('useTauriApi unwrap logic', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('unwraps successful Result<T, E> to T', async () => {
    const snapshot = {
      registryRevision: 'registry-1',
      environmentRevision: 'environment-1',
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      availability: 'available',
      projectPath: '/work/app',
      agents: {},
    };
    mockCommands.listAgents.mockResolvedValue({ status: 'ok', data: snapshot });
    const result = await listAgents(context);
    expect(result).toEqual(snapshot);
    expect(mockCommands.listAgents).toHaveBeenCalledWith(context);
  });

  it('throws error from Result<T, E> when status is error', async () => {
    const appError = { kind: 'io', data: { message: 'file not found' } };
    mockCommands.listAgents.mockResolvedValue({ status: 'error', error: appError });
    await expect(listAgents(context)).rejects.toEqual(appError);
  });

  it('passes explicit context to listSkills', async () => {
    mockCommands.listSkills.mockResolvedValue({
      status: 'ok',
      data: { skills: [], agents: [], pathExists: true },
    });
    await listSkills(context);
    expect(mockCommands.listSkills).toHaveBeenCalledWith(context);
  });

  it('passes explicit context to context-sensitive read commands', async () => {
    mockCommands.readSkillContent.mockResolvedValue({ status: 'ok', data: '# Toolkit' });
    mockCommands.listEveInstallTargets.mockResolvedValue({ status: 'ok', data: [] });

    await expect(readSkillContent({ context, skillName: 'toolkit' }))
      .resolves.toBe('# Toolkit');
    await expect(listEveInstallTargets(context)).resolves.toEqual([]);

    expect(mockCommands.readSkillContent).toHaveBeenCalledWith(
      { context, skillName: 'toolkit' },
    );
    expect(mockCommands.listEveInstallTargets).toHaveBeenCalledWith(context);
  });

  it('saves defaults against the runtime registry revision that was displayed', async () => {
    const defaults = { global: ['my-agent'], project: [] };
    mockCommands.saveDefaultTargetAgents.mockResolvedValue({
      status: 'ok',
      data: null,
    });

    await saveDefaultTargetAgents(context, defaults, 'registry-1');

    expect(mockCommands.saveDefaultTargetAgents).toHaveBeenCalledWith(
      context,
      defaults,
      'registry-1',
    );
  });

  it('unwraps the grouped update result contract without exposing payload handles', async () => {
    const response = { sources: [], skills: [], outcome: 'succeeded' };
    const execution = {
      request: { context, skillNames: ['test-skill'] },
      overwritePrivateEntries: [],
    };
    mockCommands.updateSkill.mockResolvedValue({ status: 'ok', data: response });
    const result = await updateSkill(execution, previewToken);
    expect(result).toEqual(response);
    expect(mockCommands.updateSkill).toHaveBeenCalledWith(execution, previewToken);
  });

  it('passes the backend-authoritative update check request unchanged', async () => {
    const request = { context, mode: 'force', selection: { kind: 'all' } } as const;
    const response = { sources: [], skills: [] };
    mockCommands.checkUpdates.mockResolvedValue({ status: 'ok', data: response });

    await expect(checkUpdates(request)).resolves.toEqual(response);

    expect(mockCommands.checkUpdates).toHaveBeenCalledWith(request);
  });

  it('routes install preview and execute through the same canonical request', async () => {
    const request: InstallRequest = {
      context,
      source: 'owner/repo',
      discoverySession: {
        sessionId: 'discovery-1',
        environment: context.environment,
        sourceFingerprint: 'source-1',
        expiresAtEpochMs: 1000,
      },
      payloads: [],
      skills: ['demo'],
      agentIntents: [{
        agentId: 'my-agent',
        privateEntry: 'required',
        adapterTargets: [],
      }],
      requestedMode: 'copy',
      acknowledgeRisk: true,
    };
    const preview = { token: previewToken, skills: [] };
    const response = { units: [] };
    mockCommands.previewInstall.mockResolvedValue({ status: 'ok', data: preview });
    mockCommands.installSkills.mockResolvedValue({ status: 'ok', data: response });

    await expect(previewInstall(request)).resolves.toEqual(preview);
    await expect(installSkills(request, previewToken)).resolves.toEqual(response);

    expect(mockCommands.previewInstall).toHaveBeenCalledWith(request);
    expect(mockCommands.installSkills).toHaveBeenCalledWith(request, previewToken);
  });

  it('maps host picker paths through the selected environment', async () => {
    const environment = { kind: 'wsl', distro_name: 'Ubuntu' } as const;
    mockCommands.mapEnvironmentPath.mockResolvedValue({
      status: 'ok',
      data: '/home/me/app',
    });

    await expect(mapEnvironmentPath(
      environment,
      '\\\\wsl.localhost\\Ubuntu\\home\\me\\app',
    )).resolves.toBe('/home/me/app');
    expect(mockCommands.mapEnvironmentPath).toHaveBeenCalledWith(
      environment,
      '\\\\wsl.localhost\\Ubuntu\\home\\me\\app',
    );
  });

  it('persists cross-storage warning suppression through the environment command', async () => {
    const environment = { kind: 'wsl', distro_name: 'Ubuntu' } as const;
    mockCommands.setEnvironmentProjectCrossStorageWarning.mockResolvedValue({
      status: 'ok',
      data: [],
    });

    await setEnvironmentProjectCrossStorageWarning(environment, 'project-1', true);

    expect(mockCommands.setEnvironmentProjectCrossStorageWarning).toHaveBeenCalledWith(
      environment,
      'project-1',
      true,
    );
  });

  it('opens the wizard with one required context identity', async () => {
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

  it('queries and focuses the active wizard through the generated contract', async () => {
    const snapshot = { revision: 3, active: true };
    mockCommands.getInstallWizardSession.mockResolvedValue(snapshot);
    mockCommands.focusInstallWizard.mockResolvedValue({ status: 'ok', data: true });

    await expect(getInstallWizardSession()).resolves.toEqual(snapshot);
    await expect(focusInstallWizard()).resolves.toBe(true);

    expect(mockCommands.getInstallWizardSession).toHaveBeenCalledWith();
    expect(mockCommands.focusInstallWizard).toHaveBeenCalledWith();
  });

  it('returns the direct settings snapshot generated by the command', async () => {
    const snapshot = {
      registryRevision: 'registry-1',
      activeBuiltin: [],
      activeCustom: [],
      disabledConflicts: [],
      invalidCustomRecords: [],
      currentEnvironment: context.environment,
      customStorageIssue: null,
    };
    mockCommands.getAgentSettingsSnapshot.mockResolvedValue(snapshot);

    await expect(getAgentSettingsSnapshot(context)).resolves.toEqual(snapshot);
    expect(mockCommands.getAgentSettingsSnapshot).toHaveBeenCalledWith(context);
  });

  it('unwraps every generated custom-agent management Result', async () => {
    const draft: CustomAgentDefinition = {
      id: 'my-agent',
      displayName: 'My Agent',
      global: { enabled: true, location: 'private', privatePath: { kind: 'based', base: 'home', relativePath: '.my-agent/skills' } },
      project: { enabled: false, location: 'private', privatePath: null },
      detectionPaths: [],
    };
    const settings = {
      registryRevision: 'registry-2',
      activeBuiltin: [],
      activeCustom: [],
      disabledConflicts: [],
      invalidCustomRecords: [],
      currentEnvironment: context.environment,
      customStorageIssue: null,
    };
    const impact = {
      agentId: draft.id,
      displayName: draft.displayName,
      registryRevision: 'registry-1',
      environmentRevision: 'environment-1',
      scopes: [],
      losesManagementCapability: false,
      filesWillBeDeleted: false,
    };
    const deletion = { settings, warnings: [] };
    mockCommands.validateCustomAgentDraft.mockResolvedValue({
      status: 'ok',
      data: { registryRevision: 'registry-1', environmentRevision: 'environment-1', environment: context.environment, resolved: {} },
    });
    mockCommands.saveCustomAgent.mockResolvedValue({ status: 'ok', data: settings });
    mockCommands.previewCustomAgentDelete.mockResolvedValue({ status: 'ok', data: impact });
    mockCommands.deleteCustomAgent.mockResolvedValue({ status: 'ok', data: deletion });
    mockCommands.deleteInvalidCustomAgent.mockResolvedValue({ status: 'ok', data: deletion });

    await expect(validateCustomAgentDraft(context, draft)).resolves.toMatchObject({
      registryRevision: 'registry-1',
    });
    await expect(saveCustomAgent(context, draft, null, 'registry-1')).resolves.toEqual(settings);
    await expect(previewCustomAgentDelete(context, 'my-agent', 'registry-1')).resolves.toEqual(impact);
    await expect(deleteCustomAgent(context, 'my-agent', 'registry-1')).resolves.toEqual(deletion);
    await expect(deleteInvalidCustomAgent(context, 0, 'registry-1')).resolves.toEqual(deletion);

    expect(mockCommands.validateCustomAgentDraft).toHaveBeenCalledWith(context, draft);
    expect(mockCommands.saveCustomAgent).toHaveBeenCalledWith(context, draft, null, 'registry-1');
    expect(mockCommands.previewCustomAgentDelete).toHaveBeenCalledWith(context, 'my-agent', 'registry-1');
    expect(mockCommands.deleteCustomAgent).toHaveBeenCalledWith(context, 'my-agent', 'registry-1');
    expect(mockCommands.deleteInvalidCustomAgent).toHaveBeenCalledWith(context, 0, 'registry-1');
  });

  it('throws management command errors without changing their generated shape', async () => {
    const error = { kind: 'staleRegistryRevision', expected: 'old', actual: 'new' };
    mockCommands.deleteCustomAgent.mockResolvedValue({ status: 'error', error });

    await expect(deleteCustomAgent(context, 'my-agent', 'old')).rejects.toEqual(error);
  });

  it('uses opaque Recovery IDs and revisioned cleanup confirmation', async () => {
    const status = {
      resourceId: 'recovery-1', state: 'consistentCanCleanup', revision: 'revision-1',
      environment: context.environment, displayPaths: [],
    };
    mockCommands.getRecoveryResourceStatus.mockResolvedValue({ status: 'ok', data: status });
    mockCommands.confirmRecoveryResourceResolved.mockResolvedValue({ status: 'ok', data: null });
    mockCommands.openRecoveryResource.mockResolvedValue({ status: 'ok', data: null });

    await expect(getRecoveryResourceStatus('recovery-1')).resolves.toEqual(status);
    await expect(openRecoveryResource('recovery-1')).resolves.toBeUndefined();
    await expect(confirmRecoveryResourceResolved('recovery-1', 'revision-1')).resolves.toBeUndefined();

    expect(mockCommands.confirmRecoveryResourceResolved).toHaveBeenCalledWith('recovery-1', 'revision-1');
  });
});
