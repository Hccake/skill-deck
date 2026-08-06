import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { TFunction } from 'i18next';
import type { ProjectInfo } from '@/bindings';
import { useEnvironmentStore } from '@/stores/environment';
import { projectWorkspace } from '@/stores/projects';
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
  let projectsByEnvironment: Record<string, ProjectInfo[]>;

  beforeEach(() => {
    vi.restoreAllMocks();
    projectsByEnvironment = {
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
      'wsl:ubuntu': [{
        binding: {
          id: 'windows-project',
          nativePath: '/mnt/c/Code/app',
          displayName: 'app',
          order: null,
          suppressCrossStorageWarning: false,
        },
        storage: { access: 'crossStorage', owner: { kind: 'host' } },
      }],
    };
    vi.spyOn(projectWorkspace, 'getSnapshot').mockImplementation((environment) => ({
      environment,
      phase: 'ready',
      projects: projectsByEnvironment[environment.kind === 'host' ? 'host' : 'wsl:ubuntu'] ?? [],
      error: null,
      completeness: 'complete',
      environmentRevision: 1,
      lastAttemptAt: 1,
      lastSuccessAt: 1,
      freshUntil: 300_001,
      version: 1,
    }));
    useEnvironmentStore.setState({
      environments: [
        { environment: { kind: 'host' }, displayName: 'Windows', status: 'available', revision: 1, error: null },
        {
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          displayName: 'Ubuntu 24.04',
          status: 'available',
          revision: 1,
          error: null,
        },
      ],
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

    projectsByEnvironment = {
      'wsl:ubuntu': [{
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
    };
    const context = {
      environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      scope: { scope: 'project' as const, project_id: 'native-project' },
    };
    expect(appendCrossStorageFailureGuidance('Permission denied', context, 'delete', t))
      .toBe('Permission denied');
  });
});
