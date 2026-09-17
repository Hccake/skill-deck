import type { DisplayPathStyle, ResourceLocator, ScopePathBase, SkillLocation } from '@/bindings';
import { sameEnvironment } from '@/lib/context';

// 只用于显示缩写，不参与目标身份、操作资格或写入路径判断。
function components(value: string, style: DisplayPathStyle): string[] | null {
  let path = value;
  if (style === 'windows') {
    path = path.replace(/\//g, '\\')
      .replace(/^\\\\\?\\UNC\\/i, '\\\\')
      .replace(/^(?:\\\\\?\\|\\\?\?\\)/, '');
    if (!/^(?:[a-z]:\\|\\\\[^\\]+\\[^\\]+)/i.test(path)) return null;
  } else if (!path.startsWith('/')) {
    return null;
  }
  const parts = path.split(style === 'windows' ? /\\+/ : /\/+/).filter((part) => part && part !== '.');
  return parts.includes('..') ? null : parts;
}

export function presentInstallPath(
  path: ResourceLocator,
  base: ScopePathBase | null | undefined,
  scope: SkillLocation['scope'],
): { label: string; outside: boolean } {
  const full = { label: path.nativePath, outside: false };
  if (!base || !sameEnvironment(path.environment, base.logicalRoot.environment)) return full;
  const parts = components(path.nativePath, base.pathStyle);
  if (!parts) return full;
  const compare = (part: string) => base.pathStyle === 'windows' ? part.toLowerCase() : part;
  for (const root of [base.logicalRoot, base.physicalRoot]) {
    if (!root || !sameEnvironment(path.environment, root.environment)) continue;
    const rootParts = components(root.nativePath, base.pathStyle);
    if (!rootParts) continue;
    if (rootParts.length <= parts.length && rootParts.every((part, index) => compare(part) === compare(parts[index]))) {
      const separator = base.pathStyle === 'windows' ? '\\' : '/';
      const relative = parts.slice(rootParts.length).join(separator);
      return { label: scope === 'global' ? `~${relative ? separator + relative : ''}` : relative || '.', outside: false };
    }
  }
  // 没有实际根时，无法区分目录别名与范围外的位置，保留完整地址即可。
  return { ...full, outside: base.physicalRoot != null };
}
