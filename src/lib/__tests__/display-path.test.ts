import { describe, expect, it } from 'vitest';
import { displayPath } from '../display-path';

describe('displayPath', () => {
  it.each([
    ['\\\\?\\C:\\Users\\cheng\\skills\\demo', 'C:\\Users\\cheng\\skills\\demo'],
    ['\\\\?\\UNC\\server\\share\\demo', '\\\\server\\share\\demo'],
    ['\\\\?\\unc\\Server\\Share\\demo', '\\\\Server\\Share\\demo'],
    ['D:\\Skills\\Demo', 'D:\\Skills\\Demo'],
    ['\\\\server\\share\\demo', '\\\\server\\share\\demo'],
    ['/home/alice/skills/a\\b', '/home/alice/skills/a\\b'],
    ['/Users/alice/skills/demo', '/Users/alice/skills/demo'],
    ['\\\\?\\Volume{123}\\demo', '\\\\?\\Volume{123}\\demo'],
    ['\\\\.\\PhysicalDrive0', '\\\\.\\PhysicalDrive0'],
  ])('presents %s without changing ordinary paths or device namespaces', (path, expected) => {
    expect(displayPath(path, { kind: 'native' })).toBe(expected);
  });

  it('leaves WSL path data unchanged', () => {
    const path = '/home/alice/\\\\?\\C:\\demo';
    expect(displayPath(path, { kind: 'wsl', distro_name: 'Ubuntu' })).toBe(path);
  });
});
