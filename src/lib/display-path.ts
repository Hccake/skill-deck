import type { EnvironmentRef } from '@/bindings';

// 仅转换展示文本；原始路径继续用于身份校验和文件操作。
export function displayPath(path: string, environment: EnvironmentRef): string {
  if (environment.kind === 'wsl') return path;
  return path
    .replace(/^\\\\\?\\UNC\\(?=[^\\]+\\[^\\]+)/i, '\\\\')
    .replace(/^\\\\\?\\(?=[a-z]:\\)/i, '');
}
