import type { EnvironmentRef } from '@/bindings';

const WSL_UNC_PREFIX = /^\\\\(?:wsl\.localhost|wsl\$)\\/i;
const WSL_UNC_OWNER = /^\\\\(?:wsl\.localhost|wsl\$)\\([^\\/]+)/i;
const DRVFS_PATH = /^\/mnt\/[a-z](?:\/|$)/i;

export function isCrossStorageProject(
  environment: EnvironmentRef,
  nativePath: string,
): boolean {
  if (environment.kind === 'native') {
    return WSL_UNC_PREFIX.test(nativePath);
  }
  return DRVFS_PATH.test(nativePath);
}

export function getProjectStorageOwner(
  environment: EnvironmentRef,
  nativePath: string,
): EnvironmentRef | null {
  if (environment.kind === 'native') {
    const distroName = nativePath.match(WSL_UNC_OWNER)?.[1];
    return distroName ? { kind: 'wsl', distro_name: distroName } : null;
  }
  return DRVFS_PATH.test(nativePath) ? { kind: 'native' } : null;
}
