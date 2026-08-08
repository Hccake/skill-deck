import type {
  AgentDefinition,
  CustomPathSpec,
  CustomAgentDefinition,
  CustomScopeDefinition,
  PathSpec,
  ResolvedAgent,
  ScopeDefinition,
} from '@/bindings';

export type ScopeReadMode = 'unsupported' | 'standard' | 'private' | 'both';

export interface AgentListItem {
  definition: AgentDefinition;
  customDefinition?: CustomAgentDefinition;
  runtime?: ResolvedAgent;
}

function searchableText(item: AgentListItem): string {
  const definitionPaths = [
    item.definition.global.privatePath,
    item.definition.project.privatePath,
    ...(item.definition.detection.kind === 'anyPathExists' ? item.definition.detection.paths : []),
  ].filter((path) => path !== null).map((path) => formatPathRule(path));
  const customPaths = item.customDefinition ? [
    item.customDefinition.global.privatePath,
    item.customDefinition.project.privatePath,
    ...item.customDefinition.detectionPaths,
  ].filter((path) => path !== null).map((path) => formatPathRule(path)) : [];
  const runtimePaths = item.runtime ? [
    ...item.runtime.global.readPaths,
    ...item.runtime.project.readPaths,
  ] : [];
  return [
    item.definition.displayName,
    item.definition.id,
    ...item.definition.aliases,
    ...definitionPaths,
    ...customPaths,
    ...runtimePaths,
  ].join('\n').toLocaleLowerCase();
}

export function filterAgentItems(items: AgentListItem[], query: string): AgentListItem[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  return items
    .filter((item) => !normalizedQuery || searchableText(item).includes(normalizedQuery))
    .sort((left, right) => left.definition.displayName.localeCompare(right.definition.displayName));
}

export function scopeReadMode(
  scope: ScopeDefinition | CustomScopeDefinition,
): ScopeReadMode {
  if (!scope.enabled) return 'unsupported';
  if ('location' in scope) return scope.location;
  if (scope.readsStandard && scope.privatePath) return 'both';
  if (scope.readsStandard) return 'standard';
  return 'private';
}

export function formatPathRule(path: PathSpec | CustomPathSpec): string {
  if (path.kind === 'absolute') return path.path;
  if (path.kind === 'based') {
    const base = path.base === 'configHome'
      ? 'ConfigHome'
      : path.base === 'project' ? 'Project' : 'Home';
    return `${base} / ${path.relativePath}`;
  }
  if (path.kind === 'environmentVariable') {
    const suffix = path.relativePath ? ` / ${path.relativePath}` : '';
    return `$${path.name}${suffix} -> ${formatPathRule(path.fallback)}`;
  }
  if (path.kind === 'firstExisting') {
    return [...path.candidates, path.fallback].map(formatPathRule).join(' | ');
  }
  const base = path.kind === 'configHome'
    ? 'ConfigHome'
    : path.kind === 'project' ? 'Project' : 'Home';
  return `${base} / ${path.relativePath}`;
}

export function pathRelativeValue(path: PathSpec | CustomPathSpec | null): string | null {
  if (!path) return null;
  if (path.kind === 'absolute') return null;
  if (path.kind === 'based') return path.relativePath;
  if (path.kind === 'environmentVariable') return path.relativePath;
  if (path.kind === 'firstExisting') {
    return path.candidates.map(pathRelativeValue).find((value) => value !== null)
      ?? pathRelativeValue(path.fallback);
  }
  return path.relativePath;
}
