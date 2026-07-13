import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationStore } from '../mutation';
import type { ActiveMutation } from '@/bindings';

const mocks = vi.hoisted(() => ({
  getActiveMutation: vi.fn(),
  requestCancelActiveMutation: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getActiveMutation: (...args: unknown[]) => mocks.getActiveMutation(...args),
  requestCancelActiveMutation: (...args: unknown[]) => mocks.requestCancelActiveMutation(...args),
}));

const mutation: ActiveMutation = {
  kind: 'copy',
  context: {
    environment: { kind: 'wsl', distro_name: 'Ubuntu' },
    scope: { scope: 'project', project_id: 'project-1' },
  },
  statusText: 'Copying',
  cancelable: true,
};

describe('useMutationStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({ activeMutation: null, loading: false });
  });

  it('blocks writes while allowing read-only browsing', async () => {
    mocks.getActiveMutation.mockResolvedValue(mutation);

    await useMutationStore.getState().refreshMutation();

    expect(useMutationStore.getState().isWriteBlocked()).toBe(true);
    expect(useMutationStore.getState().canBrowse()).toBe(true);
  });

  it('clears the write block after the mutation finishes', async () => {
    mocks.getActiveMutation.mockResolvedValue(null);

    await useMutationStore.getState().refreshMutation();

    expect(useMutationStore.getState().isWriteBlocked()).toBe(false);
    expect(useMutationStore.getState().canBrowse()).toBe(true);
  });
});
