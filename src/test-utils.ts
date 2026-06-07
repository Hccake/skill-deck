// src/test-utils.ts
// Mock @tauri-apps/api/core to prevent Tauri runtime errors in test environment
import { vi } from 'vitest';
import type { AgentScopeTarget } from '@/bindings';

// Mock the tauri invoke mechanism used by bindings.ts
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  Channel: vi.fn(),
}));

// Mock i18next to avoid initialization issues in tests
vi.mock('@/i18n', () => ({
  default: {
    t: (key: string) => key,
    changeLanguage: vi.fn(),
  },
}));

export function makeAgentScopeTarget(
  target: Pick<AgentScopeTarget, 'path'> & Partial<AgentScopeTarget>,
): AgentScopeTarget {
  const availability = target.availability ?? (target.automatic ? 'shared-compatible' : 'private-required');
  return {
    supported: target.supported ?? true,
    automatic: target.automatic ?? false,
    path: target.path,
    availability,
    defaultAvailable: target.defaultAvailable ?? target.automatic ?? false,
    sharedPath: target.sharedPath ?? '~/.agents/skills',
    installPath: target.installPath ?? target.path,
    readPaths: target.readPaths ?? [target.path],
    privatePath: target.privatePath ?? (availability === 'private-required' ? target.path : null),
  };
}
