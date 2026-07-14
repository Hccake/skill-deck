import { beforeEach, describe, expect, it } from 'vitest';
import type { TFunction } from 'i18next';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';
import {
  appendCrossStorageFailureGuidance,
  getCrossStorageFailureGuidance,
} from '../cross-storage-guidance';

const t = ((key: string, values?: Record<string, unknown>) => {
  if (key === 'crossStorage.failureGuidance') {
    return `${values?.operation} -> ${values?.environment}`;
  }
  return key;
}) as TFunction;

describe('cross-storage failure guidance', () => {
  beforeEach(() => {
    useEnvironmentStore.setState({
      environments: [
        { environment: { kind: 'host' }, displayName: 'Windows', status: 'available' },
        {
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          displayName: 'Ubuntu 24.04',
          status: 'available',
        },
      ],
    });
    useProjectStore.setState({
      projectsByEnvironment: {
        host: [{
          binding: {
            id: 'wsl-project',
            nativePath: '\\\\wsl.localhost\\Ubuntu\\home\\alice\\app',
            displayName: 'app',
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: {
            access: 'crossStorage',
            owner: { kind: 'wsl', distro_name: 'Ubuntu' },
          },
        }],
        'wsl:Ubuntu': [{
          binding: {
            id: 'windows-project',
            nativePath: '/mnt/c/Code/app',
            displayName: 'app',
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: { access: 'crossStorage', owner: { kind: 'host' } },
        }],
      },
    });
  });

  it('suggests the host environment for a Windows project managed from WSL', () => {
    const guidance = getCrossStorageFailureGuidance({
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'windows-project' },
    }, 'update', t);

    expect(guidance).toBe('crossStorage.operation.update -> Windows');
  });

  it('suggests the owning distro for a WSL project managed from the host', () => {
    const guidance = getCrossStorageFailureGuidance({
      environment: { kind: 'host' },
      scope: { scope: 'project', project_id: 'wsl-project' },
    }, 'install', t);

    expect(guidance).toBe('crossStorage.operation.install -> Ubuntu 24.04');
  });

  it('does not change errors for global or native project operations', () => {
    expect(getCrossStorageFailureGuidance({
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    }, 'delete', t)).toBeNull();

    useProjectStore.setState({
      projectsByEnvironment: {
        'wsl:Ubuntu': [{
          binding: {
            id: 'native-project',
            nativePath: '/home/alice/app',
            displayName: 'app',
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: {
            access: 'native',
            owner: null,
          },
        }],
      },
    });
    const context = {
      environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      scope: { scope: 'project' as const, project_id: 'native-project' },
    };
    expect(appendCrossStorageFailureGuidance('Permission denied', context, 'delete', t))
      .toBe('Permission denied');
  });
});
