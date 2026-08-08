import type { SkillLocationRef, ProjectInfo } from '@/bindings';
import { describe, expect, it } from 'vitest';
import { getCopyableProjects } from '../copy-targets';

const native = { kind: 'native' as const };
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

function sourceContext(environment = native): SkillLocationRef {
  return { environment, scope: { scope: 'project', project_id: 'source' } };
}

describe('getCopyableProjects', () => {
  it('excludes the source project and completed projects in the same environment', () => {
    const result = getCopyableProjects({
      targetEnvironment: native,
      sourceContext: sourceContext(),
      projects: [project('source'), project('target'), project('completed')],
      completedProjectIds: new Set(['completed']),
    });

    expect(result.map((entry) => entry.binding.id)).toEqual(['target']);
  });

  it('keeps the source project when copying to another environment', () => {
    const result = getCopyableProjects({
      targetEnvironment: ubuntu,
      sourceContext: sourceContext(native),
      projects: [project('source'), project('target')],
      completedProjectIds: new Set(),
    });

    expect(result.map((entry) => entry.binding.id)).toEqual(['source', 'target']);
  });

  it('preserves the project store order', () => {
    const result = getCopyableProjects({
      targetEnvironment: native,
      sourceContext: { environment: native, scope: { scope: 'global' } },
      projects: [project('b'), project('a')],
      completedProjectIds: new Set(),
    });

    expect(result.map((entry) => entry.binding.id)).toEqual(['b', 'a']);
  });
});
