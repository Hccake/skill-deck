import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationStore } from '@/stores/mutation';
import type { BackendActivitySnapshot } from '@/bindings';
import { executeLifecycleAction } from '../lifecycleApi';

const mocks = vi.hoisted(() => ({
  executeLifecycleAction: vi.fn(),
}));

vi.mock('@/bindings', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/bindings')>();
  return {
    ...original,
    commands: {
      ...original.commands,
      executeLifecycleAction: mocks.executeLifecycleAction,
    },
  };
});

const blockedSnapshot: BackendActivitySnapshot = {
  revision: 2,
  lifecycle: null,
  mutation: {
    id: 'mutation-1',
    kind: 'install',
    target: { kind: 'skillLocation',
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    },
    phase: 'acquiring',
    progress: null,
    cancelable: true,
  },
};

describe('executeLifecycleAction', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      loading: false,
      cancelling: false,
    });
  });

  it('accepts the authoritative snapshot when Rust blocks the action', async () => {
    mocks.executeLifecycleAction.mockResolvedValue({
      status: 'ok',
      data: { status: 'blocked', snapshot: blockedSnapshot },
    });

    const outcome = await executeLifecycleAction('quitApplication');

    expect(outcome).toEqual({ status: 'blocked', snapshot: blockedSnapshot });
    expect(useMutationStore.getState().activeMutation?.id).toBe('mutation-1');
  });

  it('throws a typed command error without changing mutation state', async () => {
    const error = { kind: 'io', data: { message: 'exit failed' } };
    mocks.executeLifecycleAction.mockResolvedValue({ status: 'error', error });

    await expect(executeLifecycleAction('quitApplication')).rejects.toEqual(error);
    expect(useMutationStore.getState().activeMutation).toBeNull();
  });
});
