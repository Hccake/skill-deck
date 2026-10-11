import type { ProjectInfo } from '@/bindings';
import { projectPresentation } from '@/lib/projects/presentation';
import { cn } from '@/lib/utils';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';

interface ProjectIdentityProps {
  project: ProjectInfo;
  nameClassName?: string;
  pathClassName?: string;
  nameTitle?: string;
  pathTitle?: string;
  focusable?: boolean;
}

export function ProjectIdentity({
  project,
  nameClassName,
  pathClassName,
  nameTitle,
  pathTitle,
  focusable = true,
}: ProjectIdentityProps) {
  const { name, path } = projectPresentation(project);

  return (
    <>
      <OverflowTooltip content={nameTitle ?? name} always={Boolean(nameTitle && nameTitle !== name)}>
        <span tabIndex={focusable ? 0 : undefined} className={cn('block truncate', nameClassName)}>{name}</span>
      </OverflowTooltip>
      <OverflowTooltip content={pathTitle ?? path} always={Boolean(pathTitle && pathTitle !== path)}>
        <span tabIndex={focusable ? 0 : undefined} className={cn('block truncate', pathClassName)}>{path}</span>
      </OverflowTooltip>
    </>
  );
}
