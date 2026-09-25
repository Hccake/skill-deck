// src/lib/platform.ts

/**
 * 判断当前是否处于 macOS 环境
 */
export function isMac(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Macintosh|Mac OS X/i.test(navigator.userAgent);
}

/**
 * 判断当前是否处于 Windows 环境
 */
export function isWindows(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Windows/i.test(navigator.userAgent);
}

/**
 * 判断当前是否处于 Linux 环境
 */
export function isLinux(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Linux/i.test(navigator.userAgent) && !/Android/i.test(navigator.userAgent);
}
