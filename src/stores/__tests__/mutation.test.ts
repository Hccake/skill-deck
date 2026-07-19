import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useMutationStore } from '../mutation';
import type { ActiveMutation, MutationSnapshot } from '@/bindings';

const mocks = vi.hoisted(() => ({
  getActiveMutation: vi.fn(),
  requestCancelActiveMutation: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getActiveMutation: (...args: unknown[]) => mocks.getActiveMutation(...args),
  requestCancelActiveMutation: (...args: unknown[]) => mocks.requestCancelActiveMutation(...args),
}));

const mutation: ActiveMutation = {
  id: 'mutation-1',
  kind: 'copy',
  context: {
    environment: { kind: 'wsl', distro_name: 'Ubuntu' },
    scope: { scope: 'project', project_id: 'project-1' },
  },
  phase: 'acquiring',
  progress: { subject: 'toolkit', current: 1, total: 2 },
  cancelable: true,
};

const snapshot = (revision: number, active: ActiveMutation | null): MutationSnapshot => ({
  revision,
  active,
});

describe('useMutationStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMutationStore.setState({
      revision: 0,
      activeMutation: null,
      loading: false,
      cancelling: false,
    });
  });

  it('blocks writes while allowing read-only browsing', async () => {
    mocks.getActiveMutation.mockResolvedValue(snapshot(1, mutation));

    await useMutationStore.getState().refreshMutation();

    expect(useMutationStore.getState().isWriteBlocked()).toBe(true);
    expect(useMutationStore.getState().canBrowse()).toBe(true);
  });

  it('clears the write block after the mutation finishes', async () => {
    useMutationStore.setState({ revision: 1, activeMutation: mutation });
    mocks.getActiveMutation.mockResolvedValue(snapshot(2, null));

    await useMutationStore.getState().refreshMutation();

    expect(useMutationStore.getState().isWriteBlocked()).toBe(false);
    expect(useMutationStore.getState().canBrowse()).toBe(true);
  });

  it('shares one backend request between overlapping refreshes', async () => {
    let resolveMutation: ((value: MutationSnapshot) => void) | undefined;
    mocks.getActiveMutation.mockImplementation(() => new Promise((resolve) => {
      resolveMutation = resolve;
    }));

    const firstRefresh = useMutationStore.getState().refreshMutation();
    const secondRefresh = useMutationStore.getState().refreshMutation();

    expect(mocks.getActiveMutation).toHaveBeenCalledTimes(1);

    resolveMutation?.(snapshot(1, mutation));
    await Promise.all([firstRefresh, secondRefresh]);

    expect(useMutationStore.getState().activeMutation).toEqual(mutation);
    expect(useMutationStore.getState().loading).toBe(false);
  });

  it('ignores an older query result after a newer finish snapshot', async () => {
    useMutationStore.setState({ revision: 5, activeMutation: null });
    mocks.getActiveMutation.mockResolvedValue(snapshot(4, mutation));

    await useMutationStore.getState().refreshMutation();

    expect(useMutationStore.getState().revision).toBe(5);
    expect(useMutationStore.getState().activeMutation).toBeNull();
  });

  it('keeps cancellation pending until a refresh observes completion', async () => {
    useMutationStore.setState({ revision: 1, activeMutation: mutation });
    mocks.requestCancelActiveMutation.mockResolvedValue(true);
    mocks.getActiveMutation
      .mockResolvedValueOnce(snapshot(2, mutation))
      .mockResolvedValueOnce(snapshot(3, null));

    await useMutationStore.getState().cancelActiveMutation();

    expect(useMutationStore.getState().cancelling).toBe(true);

    await useMutationStore.getState().refreshMutation();

    expect(useMutationStore.getState().activeMutation).toBeNull();
    expect(useMutationStore.getState().cancelling).toBe(false);
  });
});
