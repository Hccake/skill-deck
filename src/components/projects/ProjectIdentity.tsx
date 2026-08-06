import type { ProjectInfo } from '@/bindings';
import { projectPresentation } from '@/lib/projects/presentation';
import { cn } from '@/lib/utils';

interface ProjectIdentityProps {
  project: ProjectInfo;
  nameClassName?: string;
  pathClassName?: string;
  nameTitle?: string;
  pathTitle?: string;
}

export function ProjectIdentity({
  project,
  nameClassName,
  pathClassName,
  nameTitle,
  pathTitle,
}: ProjectIdentityProps) {
  const { name, path } = projectPresentation(project);

  return (
    <>
      <span title={nameTitle ?? name} className={cn('block truncate', nameClassName)}>{name}</span>
      <span title={pathTitle ?? path} className={cn('block truncate', pathClassName)}>{path}</span>
    </>
  );
}
