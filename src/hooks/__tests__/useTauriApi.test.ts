// src/hooks/__tests__/useTauriApi.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';

const { mockCommands } = vi.hoisted(() => ({
  mockCommands: {
    listAgents: vi.fn(),
    listSkills: vi.fn(),
    installSkills: vi.fn(),
    updateSkill: vi.fn(),
    getConfig: vi.fn(),
  },
}));

vi.mock('@/bindings', () => ({
  commands: mockCommands,
}));

import { installSkills, listAgents, listSkills, updateSkill } from '../useTauriApi';

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
});
