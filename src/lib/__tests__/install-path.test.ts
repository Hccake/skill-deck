import { describe, expect, it } from 'vitest';
import type { ResourceLocator, ScopePathBase } from '@/bindings';
import { presentInstallPath } from '../install-path';

const native = { kind: 'native' } as const;
const wsl = { kind: 'wsl', distro_name: 'Ubuntu' } as const;
const locator = (nativePath: string, environment = native as ResourceLocator['environment']) => ({ environment, nativePath });
const base = (root: string, pathStyle: 'posix' | 'windows' = 'posix'): ScopePathBase => ({
  logicalRoot: locator(root), physicalRoot: locator(root), pathStyle,
});

describe('presentInstallPath', () => {
  it('uses the explicit project root and preserves the Skill directory', () => {
    expect(presentInstallPath(locator('/work/app/.claude/skills/toolkit'), base('/work/app'), 'project'))
      .toEqual({ label: '.claude/skills/toolkit', outside: false });
    expect(presentInstallPath(locator('/work/app-other/toolkit'), base('/work/app'), 'project'))
      .toEqual({ label: '/work/app-other/toolkit', outside: true });
  });

  it('uses the selected WSL home and never abbreviates another Environment', () => {
    const wslBase: ScopePathBase = { logicalRoot: locator('/home/alice', wsl), physicalRoot: locator('/home/alice', wsl), pathStyle: 'posix' };
    expect(presentInstallPath(locator('/home/alice/.claude/skills/demo', wsl), wslBase, 'global'))
      .toEqual({ label: '~/.claude/skills/demo', outside: false });
    expect(presentInstallPath(locator('/home/alice/.claude/skills/demo'), wslBase, 'global'))
      .toEqual({ label: '/home/alice/.claude/skills/demo', outside: false });
  });

  it('recognizes logical and physical roots without guessing from an installation', () => {
    const linkedBase = { ...base('/work/alias'), physicalRoot: locator('/disk/project') };
    for (const path of ['/work/alias/.agents/skills/demo', '/disk/project/.agents/skills/demo']) {
      expect(presentInstallPath(locator(path), linkedBase, 'project'))
        .toEqual({ label: '.agents/skills/demo', outside: false });
    }
    expect(presentInstallPath(locator('/other/demo'), { ...linkedBase, physicalRoot: null }, 'project'))
      .toEqual({ label: '/other/demo', outside: false });
    expect(presentInstallPath(locator('/work/.agents/skills/demo'), undefined, 'project'))
      .toEqual({ label: '/work/.agents/skills/demo', outside: false });
  });

  it.each([
    ['C:\\Users\\Alice', 'c:/users/alice/.claude/skills/demo', '.claude\\skills\\demo'],
    ['C:\\Users\\Alice', '\\\\?\\C:\\Users\\Alice\\.claude\\skills\\demo', '.claude\\skills\\demo'],
    ['\\\\Server\\Share\\App', '\\\\?\\UNC\\server\\share\\app\\.claude\\skills\\demo', '.claude\\skills\\demo'],
    ['C:\\', 'c:\\.claude\\skills\\demo', '.claude\\skills\\demo'],
  ])('handles Windows path forms: %s', (root, path, label) => {
    expect(presentInstallPath(locator(path), base(root, 'windows'), 'project'))
      .toEqual({ label, outside: false });
  });

  it('keeps different drives and UNC shares outside the base', () => {
    expect(presentInstallPath(locator('D:\\app\\demo'), base('C:\\app', 'windows'), 'project').outside).toBe(true);
    expect(presentInstallPath(locator('\\\\server\\other\\demo'), base('\\\\server\\share', 'windows'), 'global').outside).toBe(true);
  });

  it('respects POSIX case and literal backslashes, including a filesystem root base', () => {
    expect(presentInstallPath(locator('/work/App/demo'), base('/work/app'), 'project').outside).toBe(true);
    expect(presentInstallPath(locator('/work/a\\b/.claude/skills/demo'), base('/work/a\\b'), 'project').label)
      .toBe('.claude/skills/demo');
    expect(presentInstallPath(locator('/.claude/skills/demo'), base('/'), 'project').label)
      .toBe('.claude/skills/demo');
  });

  it('does not claim relative ownership for unresolved parent traversal', () => {
    expect(presentInstallPath(locator('/work/app/link/../demo'), base('/work/app'), 'project'))
      .toEqual({ label: '/work/app/link/../demo', outside: false });
  });
});
