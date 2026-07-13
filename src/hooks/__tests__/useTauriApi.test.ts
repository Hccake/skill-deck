// src/hooks/__tests__/useTauriApi.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';

const { mockCommands } = vi.hoisted(() => ({
  mockCommands: {
    listAgents: vi.fn(),
    listSkills: vi.fn(),
    installSkills: vi.fn(),
    installSkillsV2: vi.fn(),
    updateSkill: vi.fn(),
    updateSkillV2: vi.fn(),
    removeSkillV2: vi.fn(),
    getSkillAgentDetailsV2: vi.fn(),
    manageSkillAgentsV2: vi.fn(),
    cleanupDuplicateAgentCopiesV2: vi.fn(),
    copySkillToProjectsV2: vi.fn(),
    saveDefaultTargetAgentsV2: vi.fn(),
    checkOverwritesV2: vi.fn(),
    checkUpdatesV2: vi.fn(),
    updateSkillsBatchV2: vi.fn(),
    mapEnvironmentPathV2: vi.fn(),
    getConfig: vi.fn(),
  },
}));

vi.mock('@/bindings', () => ({
  commands: mockCommands,
}));

import {
  installSkills,
  installSkillsV2,
  listAgents,
  listSkills,
  mapEnvironmentPath,
  updateSkill,
  updateSkillV2,
} from '../useTauriApi';

describe('useTauriApi unwrap logic', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('unwraps successful Result<T, E> to T', async () => {
    const agents = [{ id: 'claude-code', name: 'Claude Code', detected: true }];
    mockCommands.listAgents.mockResolvedValue({ status: 'ok', data: agents });
    const result = await listAgents();
    expect(result).toEqual(agents);
  });

  it('throws error from Result<T, E> when status is error', async () => {
    const appError = { kind: 'io', data: { message: 'file not found' } };
    mockCommands.listAgents.mockResolvedValue({ status: 'error', error: appError });
    await expect(listAgents()).rejects.toEqual(appError);
  });

  it('passes parameters correctly through wrapper functions', async () => {
    mockCommands.listSkills.mockResolvedValue({
      status: 'ok',
      data: { skills: [], pathExists: true },
    });
    await listSkills({ scope: 'global' });
    expect(mockCommands.listSkills).toHaveBeenCalledWith({
      scope: 'global',
      projectPath: null,
    });
  });

  it('defaults optional params to null', async () => {
    mockCommands.listSkills.mockResolvedValue({
      status: 'ok',
      data: { skills: [], pathExists: true },
    });
    await listSkills();
    expect(mockCommands.listSkills).toHaveBeenCalledWith({
      scope: null,
      projectPath: null,
    });
  });

  it('passes projectPath when provided', async () => {
    mockCommands.listSkills.mockResolvedValue({
      status: 'ok',
      data: { skills: [], pathExists: true },
    });
    await listSkills({ scope: 'project', projectPath: '/my/project' });
    expect(mockCommands.listSkills).toHaveBeenCalledWith({
      scope: 'project',
      projectPath: '/my/project',
    });
  });

  it('passes retry flag to installSkills command', async () => {
    mockCommands.installSkills.mockResolvedValue({
      status: 'ok',
      data: {
        successful: [],
        failed: [],
        symlinkFallbackAgents: [],
      },
    });
    await installSkills({
      source: 'owner/repo',
      skills: ['skill-a'],
      agents: ['cursor'],
      scope: 'global',
      projectPath: null,
      mode: 'symlink',
      retry: true,
    });
    expect(mockCommands.installSkills).toHaveBeenCalledWith(
      expect.objectContaining({ retry: true })
    );
  });

  it('passes explicit private copy agents to installSkills command', async () => {
    mockCommands.installSkills.mockResolvedValue({
      status: 'ok',
      data: { successful: [], failed: [], symlinkFallbackAgents: [] },
    });

    await installSkills({
      source: 'owner/repo',
      skills: ['demo'],
      agents: [],
      privateCopyAgents: ['firebender'],
      scope: 'global',
      projectPath: null,
      mode: 'copy',
      retry: false,
      preserveExistingModes: false,
      acknowledgeRisk: false,
    });

    expect(mockCommands.installSkills).toHaveBeenCalledWith(
      expect.objectContaining({ privateCopyAgents: ['firebender'] })
    );
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
    const result = await updateSkill({ scope: 'global', name: 'test-skill' });
    expect(result).toEqual(response);
    expect(result.results[0].agentResults).toHaveLength(1);
  });

  it('routes explicit context operations through v2 commands', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;
    const response = { successful: [], failed: [], symlinkFallbackAgents: [] };
    mockCommands.installSkillsV2.mockResolvedValue({ status: 'ok', data: response });
    mockCommands.updateSkillV2.mockResolvedValue({ status: 'ok', data: { results: [], summary: {} } });

    await installSkillsV2(context, {
      source: 'owner/repo',
      skills: ['demo'],
      agents: [],
      privateCopyAgents: [],
      scope: 'project',
      projectPath: null,
      mode: 'copy',
      retry: false,
    });
    await updateSkillV2(context, 'demo');

    expect(mockCommands.installSkillsV2).toHaveBeenCalledWith(context, expect.objectContaining({
      skills: ['demo'],
    }));
    expect(mockCommands.updateSkillV2).toHaveBeenCalledWith(context, 'demo');
  });

  it('maps host picker paths through the selected environment', async () => {
    const environment = { kind: 'wsl', distro_name: 'Ubuntu' } as const;
    mockCommands.mapEnvironmentPathV2.mockResolvedValue({
      status: 'ok',
      data: '/home/me/app',
    });

    await expect(mapEnvironmentPath(
      environment,
      '\\\\wsl.localhost\\Ubuntu\\home\\me\\app',
    )).resolves.toBe('/home/me/app');
    expect(mockCommands.mapEnvironmentPathV2).toHaveBeenCalledWith(
      environment,
      '\\\\wsl.localhost\\Ubuntu\\home\\me\\app',
    );
  });
});
