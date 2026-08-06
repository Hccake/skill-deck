import type { ContextRef, ProjectInfo } from '@/bindings';
import { describe, expect, it } from 'vitest';
import { getCopyableProjects } from '../copy-targets';

const host = { kind: 'host' as const };
const ubuntu = { kind: 'wsl' as const, distro_name: 'Ubuntu' };

function project(id: string): ProjectInfo {
  return {
    binding: {
      id,
      nativePath: `/work/${id}`,
      displayName: null,
      order: null,
      suppressCrossStorageWarning: false,
    },
    storage: { access: 'native', owner: null },
  };
}

function sourceContext(environment = host): ContextRef {
  return { environment, scope: { scope: 'project', project_id: 'source' } };
}

describe('getCopyableProjects', () => {
  it('excludes the source project and completed projects in the same environment', () => {
    const result = getCopyableProjects({
      targetEnvironment: host,
      sourceContext: sourceContext(),
      projects: [project('source'), project('target'), project('completed')],
      completedProjectIds: new Set(['completed']),
    });

    expect(result.map((entry) => entry.binding.id)).toEqual(['target']);
  });

  it('keeps the source project when copying to another environment', () => {
    const result = getCopyableProjects({
      targetEnvironment: ubuntu,
      sourceContext: sourceContext(host),
      projects: [project('source'), project('target')],
      completedProjectIds: new Set(),
    });

    expect(result.map((entry) => entry.binding.id)).toEqual(['source', 'target']);
  });

  it('preserves the project store order', () => {
    const result = getCopyableProjects({
      targetEnvironment: host,
      sourceContext: { environment: host, scope: { scope: 'global' } },
      projects: [project('b'), project('a')],
      completedProjectIds: new Set(),
    });

    expect(result.map((entry) => entry.binding.id)).toEqual(['b', 'a']);
  });
});
