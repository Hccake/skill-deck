/* @vitest-environment jsdom */

import { describe, expect, it, vi } from 'vitest';
import { isMac, isWindows, isLinux } from '../platform';

describe('platform helpers', () => {
  it('detects macOS when userAgent includes Macintosh', () => {
    const originalUserAgent = navigator.userAgent;
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36',
    );

    expect(isMac()).toBe(true);
    expect(isWindows()).toBe(false);

    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(originalUserAgent);
  });

  it('detects Windows when userAgent includes Windows', () => {
    const originalUserAgent = navigator.userAgent;
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    );

    expect(isMac()).toBe(false);
    expect(isWindows()).toBe(true);
    expect(isLinux()).toBe(false);

    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(originalUserAgent);
  });

  it('detects Linux when userAgent includes Linux', () => {
    const originalUserAgent = navigator.userAgent;
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko)',
    );

    expect(isMac()).toBe(false);
    expect(isWindows()).toBe(false);
    expect(isLinux()).toBe(true);

    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(originalUserAgent);
  });
});
