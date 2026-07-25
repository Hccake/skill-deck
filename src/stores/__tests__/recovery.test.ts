import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RecoveryResourceStatus } from '@/bindings';

const api = vi.hoisted(() => ({
  listRecoveryResources: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => api);

import { useRecoveryStore } from '../recovery';

function resource(state: RecoveryResourceStatus['state']): RecoveryResourceStatus {
  return {
    resourceId: 'recovery-1',
    state,
    revision: `revision-${state}`,
    environment: { kind: 'wsl', distro_name: 'Ubuntu' },
    createdAtEpochMs: 123,
    displayPaths: [],
    diagnostic: null,
  };
}

describe('Recovery store', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useRecoveryStore.setState({ resources: [], state: 'idle', error: null });
  });

  it('enumerates restart recovery resources without a retained mutation result ID', async () => {
    api.listRecoveryResources.mockResolvedValue([resource('needsAttention')]);

    await useRecoveryStore.getState().load();

    expect(useRecoveryStore.getState()).toMatchObject({
      state: 'ready',
      resources: [expect.objectContaining({ resourceId: 'recovery-1' })],
    });
  });

  it('removes Missing resources after refresh but keeps EnvironmentUnavailable visible', async () => {
    api.listRecoveryResources
      .mockResolvedValueOnce([resource('needsAttention')])
      .mockResolvedValueOnce([resource('missing'), { ...resource('environmentUnavailable'), resourceId: 'recovery-2' }]);
    await useRecoveryStore.getState().load();

    await useRecoveryStore.getState().load();

    expect(useRecoveryStore.getState().resources).toEqual([
      expect.objectContaining({ resourceId: 'recovery-2', state: 'environmentUnavailable' }),
    ]);
  });

  it('shares one in-flight enumeration across concurrent refresh triggers', async () => {
    let resolve!: (resources: RecoveryResourceStatus[]) => void;
    api.listRecoveryResources.mockReturnValue(new Promise((done) => {
      resolve = done;
    }));

    const first = useRecoveryStore.getState().load();
    const second = useRecoveryStore.getState().load();

    expect(api.listRecoveryResources).toHaveBeenCalledTimes(1);
    resolve([resource('needsAttention')]);
    await Promise.all([first, second]);
    expect(useRecoveryStore.getState().resources).toHaveLength(1);
  });

  it('keeps the last load error visible until a refresh succeeds', async () => {
    api.listRecoveryResources.mockRejectedValueOnce(new Error('read failed'));
    await useRecoveryStore.getState().load();
    expect(useRecoveryStore.getState().error).not.toBeNull();

    let resolve!: (resources: RecoveryResourceStatus[]) => void;
    api.listRecoveryResources.mockReturnValueOnce(new Promise((done) => {
      resolve = done;
    }));
    const refresh = useRecoveryStore.getState().load();

    expect(useRecoveryStore.getState()).toMatchObject({
      state: 'loading',
      error: expect.anything(),
    });

    resolve([]);
    await refresh;
    expect(useRecoveryStore.getState()).toMatchObject({
      state: 'ready',
      error: null,
    });
  });
});
