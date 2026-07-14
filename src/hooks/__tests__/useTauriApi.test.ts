// src/hooks/__tests__/useTauriApi.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';

const { mockCommands } = vi.hoisted(() => ({
  mockCommands: {
    listAgents: vi.fn(),
    listSkills: vi.fn(),
    listEveInstallTargets: vi.fn(),
    readSkillContent: vi.fn(),
    installSkills: vi.fn(),
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
    getConfig: vi.fn(),
  },
}));

vi.mock('@/bindings', () => ({
  commands: mockCommands,
}));

import {
  installSkills,
  listAgents,
  listEveInstallTargets,
  listSkills,
  mapEnvironmentPath,
  openInstallWizard,
  readSkillContent,
  setEnvironmentProjectCrossStorageWarning,
  updateSkill,
} from '../useTauriApi';

const context = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  scope: { scope: 'project', project_id: 'project-1' },
} as const;

describe('useTauriApi unwrap logic', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('unwraps successful Result<T, E> to T', async () => {
    const agents = [{ id: 'claude-code', name: 'Claude Code', detected: true }];
    mockCommands.listAgents.mockResolvedValue({ status: 'ok', data: agents });
    const result = await listAgents(context);
    expect(result).toEqual(agents);
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
      data: { skills: [], pathExists: true },
    });
    await listSkills(context);
    expect(mockCommands.listSkills).toHaveBeenCalledWith(context);
  });

  it('passes explicit context to context-sensitive read commands', async () => {
    mockCommands.readSkillContent.mockResolvedValue({ status: 'ok', data: '# Toolkit' });
    mockCommands.listEveInstallTargets.mockResolvedValue({ status: 'ok', data: [] });

    await expect(readSkillContent(context, '/work/app/.agents/skills/toolkit'))
      .resolves.toBe('# Toolkit');
    await expect(listEveInstallTargets(context)).resolves.toEqual([]);

    expect(mockCommands.readSkillContent).toHaveBeenCalledWith(
      context,
      '/work/app/.agents/skills/toolkit',
    );
    expect(mockCommands.listEveInstallTargets).toHaveBeenCalledWith(context);
  });

  it('unwraps updateSkill response with structured results', async () => {
    const response = {
      results: [
        {
          name: 'test-skill',
          status: 'success',
          warnings: [],
          agentResults: [
            { agent: 'cursor', status: 'success', durationMs: 5 },
          ],
        },
      ],
      summary: { total: 1, succeeded: 1, partial: 0, failed: 0, skipped: 0 },
    };
    mockCommands.updateSkill.mockResolvedValue({ status: 'ok', data: response });
    const result = await updateSkill(context, 'test-skill');
    expect(result).toEqual(response);
    expect(result.results[0].agentResults).toHaveLength(1);
    expect(mockCommands.updateSkill).toHaveBeenCalledWith(context, 'test-skill');
  });

  it('routes explicit context operations through canonical commands', async () => {
    const response = { successful: [], failed: [], symlinkFallbackAgents: [] };
    mockCommands.installSkills.mockResolvedValue({ status: 'ok', data: response });
    mockCommands.updateSkill.mockResolvedValue({ status: 'ok', data: { results: [], summary: {} } });

    await installSkills(context, {
      source: 'owner/repo',
      skills: ['demo'],
      agents: [],
      privateCopyAgents: [],
      scope: 'project',
      projectPath: null,
      mode: 'copy',
      retry: false,
    });
    await updateSkill(context, 'demo');

    expect(mockCommands.installSkills).toHaveBeenCalledWith(context, expect.objectContaining({
      skills: ['demo'],
    }));
    expect(mockCommands.updateSkill).toHaveBeenCalledWith(context, 'demo');
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
});
