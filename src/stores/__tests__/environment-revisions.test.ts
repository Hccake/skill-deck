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
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
      wslCapabilityRevision: 3,
      discoveryState: 'idle',
      discoveryError: null,
      discoveryCompletedAt: null,
    });
  });

  it('rejects runtime events from an earlier WSL enable cycle', () => {
    useEnvironmentStore.setState({
      environments: [host, ubuntu],
      runtimeByEnvironment: { host, 'wsl:ubuntu': ubuntu },
    });
    const staleCycle: EnvironmentRuntimeEvent = {
      capabilityRevision: 2,
      revision: 99,
      environment: ubuntu.environment,
      status: 'unavailable',
      error: null,
    };

    useEnvironmentStore.getState().applyRuntimeEvent(staleCycle);

    expect(useEnvironmentStore.getState().environments[1]).toEqual(ubuntu);
  });

  it('rejects older events and older discovery snapshots per Environment', async () => {
    api.listEnvironments.mockResolvedValue({
      environments: [host, ubuntu],
      error: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
      wslCapabilityRevision: 3,
    });
    await useEnvironmentStore.getState().discover();

    const newer: EnvironmentRuntimeEvent = {
      capabilityRevision: 3,
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
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
      wslCapabilityRevision: 3,
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
      capabilityRevision: 3,
      revision: 9,
      environment: ubuntu.environment,
      status: 'connecting',
      error: null,
    };
    useEnvironmentStore.getState().applyRuntimeEvent(event);
    api.listEnvironments.mockResolvedValue({
      environments: [host, ubuntu],
      error: null,
      wslIntegrationSupported: true,
      wslIntegrationEnabled: true,
      wslCapabilityRevision: 3,
    });

    await useEnvironmentStore.getState().discover();

    expect(useEnvironmentStore.getState().environments[1]).toMatchObject({
      status: 'connecting',
      revision: 9,
    });
  });
});
