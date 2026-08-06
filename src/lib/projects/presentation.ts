import type { ProjectBinding, ProjectInfo } from '@/bindings';

export interface ProjectPresentation {
  name: string;
  path: string;
}

export function projectPathBasename(path: string): string {
  const normalizedPath = path.replace(/\\/g, '/');
  const trimmedPath = normalizedPath.replace(/\/+$/, '');

  if (
    trimmedPath.length === 0
    || /^[A-Za-z]:$/.test(trimmedPath)
    || /^\/{2}[^/]+\/[^/]+$/.test(trimmedPath)
  ) {
    return path;
  }

  return trimmedPath.split('/').at(-1) || path;
}

export function projectBindingDisplayName(binding: Pick<ProjectBinding, 'displayName' | 'nativePath'>): string {
  const configuredName = binding.displayName?.trim();
  if (configuredName) return configuredName;
  return projectPathBasename(binding.nativePath);
}

export function projectDisplayName(project: ProjectInfo): string {
  return projectBindingDisplayName(project.binding);
}

export function projectPresentation(project: ProjectInfo): ProjectPresentation {
  return {
    name: projectDisplayName(project),
    path: project.binding.nativePath,
  };
}
