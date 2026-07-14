import { describe, expect, it } from 'vitest';
import { getProjectStorageOwner, isCrossStorageProject } from '../projectStorage';

describe('isCrossStorageProject', () => {
  it('detects WSL storage managed from the host environment', () => {
    expect(isCrossStorageProject(
      { kind: 'host' },
      '\\\\wsl.localhost\\Ubuntu\\home\\alice\\app',
    )).toBe(true);
    expect(isCrossStorageProject({ kind: 'host' }, 'C:\\Code\\app')).toBe(false);
    expect(isCrossStorageProject({ kind: 'host' }, '/home/alice/app')).toBe(false);
  });

  it('detects Windows DrvFS storage managed from a WSL environment', () => {
    const ubuntu = { kind: 'wsl', distro_name: 'Ubuntu' } as const;

    expect(isCrossStorageProject(ubuntu, '/mnt/c/Code/app')).toBe(true);
    expect(isCrossStorageProject(ubuntu, '/mnt/D/Code/app')).toBe(true);
    expect(isCrossStorageProject(ubuntu, '/home/alice/app')).toBe(false);
    expect(isCrossStorageProject(ubuntu, '/mnt/wsl/shared-distros/app')).toBe(false);
  });

  it('identifies the environment that owns cross-storage project files', () => {
    expect(getProjectStorageOwner(
      { kind: 'host' },
      '\\\\wsl.localhost\\Ubuntu-24.04\\home\\alice\\app',
    )).toEqual({ kind: 'wsl', distro_name: 'Ubuntu-24.04' });
    expect(getProjectStorageOwner(
      { kind: 'wsl', distro_name: 'Ubuntu' },
      '/mnt/c/Code/app',
    )).toEqual({ kind: 'host' });
    expect(getProjectStorageOwner(
      { kind: 'wsl', distro_name: 'Ubuntu' },
      '/home/alice/app',
    )).toBeNull();
  });
});
