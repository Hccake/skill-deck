import type { ProjectInfo } from '@/bindings';
import { describe, expect, it } from 'vitest';
import { projectDisplayName, projectPresentation } from '../presentation';

function project(nativePath: string, displayName: string | null = null): ProjectInfo {
  return {
    binding: {
      id: nativePath,
      nativePath,
      displayName,
      order: null,
      suppressCrossStorageWarning: false,
    },
    storage: { access: 'native', owner: null },
  };
}

describe('project presentation', () => {
  it('prefers a configured display name and trims surrounding whitespace', () => {
    expect(projectDisplayName(project('/work/app', '  Team App  '))).toBe('Team App');
  });

  it.each([
    ['/work/app', 'app'],
    ['C:\\Code\\app', 'app'],
    ['\\\\server\\share\\app\\', 'app'],
  ])('falls back to the cross-platform basename for %s', (nativePath, expected) => {
    expect(projectDisplayName(project(nativePath))).toBe(expected);
  });

  it('keeps the original path as the secondary display value', () => {
    const value = projectPresentation(project('C:\\Code\\app\\', 'App'));
    expect(value).toEqual({ name: 'App', path: 'C:\\Code\\app\\' });
  });

  it('falls back to the original path for a root path', () => {
    expect(projectDisplayName(project('/'))).toBe('/');
  });

  it.each([
    ['C:\\', 'C:\\'],
    ['\\\\server\\share\\', '\\\\server\\share\\'],
  ])('keeps the original path for a root path: %s', (nativePath, expected) => {
    expect(projectDisplayName(project(nativePath))).toBe(expected);
  });
});
