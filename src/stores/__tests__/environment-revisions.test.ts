import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentInfo, EnvironmentRuntimeEvent } from '@/bindings';

const api = vi.hoisted(() => ({
  listEnvironments: vi.fn(),
  connectEnvironment: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => api);

import { useEnvironmentStore } from '../environment';

const host: EnvironmentInfo = {
  environment: { kind: 'host' },
  displayName: 'Windows',
  status: 'available',
  revision: 1,
  error: null,
};
const ubuntu: EnvironmentInfo = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  displayName: 'Ubuntu',
  status: 'available',
  revision: 5,
  error: null,
};

describe('Environment revision convergence', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useEnvironmentStore.setState({
      environments: [],
      runtimeByEnvironment: {},
      discoveryState: 'idle',
      discoveryError: null,
      errorsByEnvironment: {},
      discoveryCompletedAt: null,
    });
  });

  it('rejects older events and older discovery snapshots per Environment', async () => {
    api.listEnvironments.mockResolvedValue({ environments: [host, ubuntu], error: null });
    await useEnvironmentStore.getState().discover();

    const newer: EnvironmentRuntimeEvent = {
      revision: 7,
      environment: ubuntu.environment,
      status: 'unavailable',
      error: {
        kind: 'environmentUnavailable',
        data: { environment: ubuntu.environment, message: 'stopped' },
      },
    };
    useEnvironmentStore.getState().applyRuntimeEvent(newer);
    useEnvironmentStore.getState().applyRuntimeEvent({ ...newer, revision: 6, status: 'available', error: null });

    api.listEnvironments.mockResolvedValue({
      environments: [host, { ...ubuntu, revision: 6 }],
      error: null,
    });
    useEnvironmentStore.setState({ discoveryCompletedAt: null });
    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState().environments[1]).toMatchObject({
      status: 'unavailable',
      revision: 7,
      error: newer.error,
    });
  });

  it('retains an event that races ahead of initial discovery and merges it when inventory arrives', async () => {
    const event: EnvironmentRuntimeEvent = {
      revision: 9,
      environment: ubuntu.environment,
      status: 'connecting',
      error: null,
    };
    useEnvironmentStore.getState().applyRuntimeEvent(event);
    api.listEnvironments.mockResolvedValue({ environments: [host, ubuntu], error: null });

    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState().environments[1]).toMatchObject({
      status: 'connecting',
      revision: 9,
    });
  });
});
