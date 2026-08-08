import { describe, expect, it } from 'vitest';
import type { SkillLocationRef, EnvironmentRef } from '@/bindings';
import {
  contextKey,
  environmentKey,
  globalContext,
  sameContext,
  sameEnvironment,
} from '../context';

const native: EnvironmentRef = { kind: 'native' };
const ubuntu: EnvironmentRef = { kind: 'wsl', distro_name: 'Ubuntu 24.04/dev' };

describe('context identity', () => {
  it('builds stable encoded environment and context keys', () => {
    expect(environmentKey(native)).toBe('native');
    expect(environmentKey(ubuntu)).toBe('wsl:ubuntu%2024.04%2Fdev');
    expect(contextKey(globalContext(native))).toBe('native/global');
    expect(contextKey({
      environment: ubuntu,
      scope: { scope: 'project', project_id: 'team/app:frontend' },
    })).toBe('wsl:ubuntu%2024.04%2Fdev/project:team%2Fapp%3Afrontend');
  });

  it('constructs a global context without changing the environment identity', () => {
    expect(globalContext(ubuntu)).toEqual({
      environment: ubuntu,
      scope: { scope: 'global' },
    });
  });

  it('compares environments and contexts by their domain identity', () => {
    const project: SkillLocationRef = {
      environment: ubuntu,
      scope: { scope: 'project', project_id: 'project-a' },
    };

    expect(sameEnvironment(ubuntu, { ...ubuntu })).toBe(true);
    expect(sameEnvironment(ubuntu, { kind: 'wsl', distro_name: 'Debian' })).toBe(false);
    expect(sameContext(project, {
      environment: { ...ubuntu },
      scope: { scope: 'project', project_id: 'project-a' },
    })).toBe(true);
    expect(sameContext(project, globalContext(ubuntu))).toBe(false);
  });

  it('normalizes WSL distro casing without changing the display name', () => {
    const lowercase = { kind: 'wsl' as const, distro_name: 'ubuntu 24.04/dev' };

    expect(environmentKey(ubuntu)).toBe('wsl:ubuntu%2024.04%2Fdev');
    expect(sameEnvironment(ubuntu, lowercase)).toBe(true);
    expect(globalContext(ubuntu).environment).toEqual(ubuntu);
  });
});
