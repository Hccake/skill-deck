// src/hooks/__tests__/useTauriApi.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';

const { mockCommands } = vi.hoisted(() => ({
  mockCommands: {
    listAgents: vi.fn(),
    listSkills: vi.fn(),
    getConfig: vi.fn(),
  },
}));

vi.mock('@/bindings', () => ({
  commands: mockCommands,
}));

import { listAgents, listSkills } from '../useTauriApi';

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
});
