// src/components/skills/add-skill/ScopeBadge.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Globe, Folder } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import type { EnvironmentRef } from '@/bindings';
import { environmentRefDisplayName } from '@/lib/environments/presentation';
import { projectPathBasename } from '@/lib/projects/presentation';

interface ScopeBadgeProps {
  scope: 'global' | 'project';
  projectPath?: string;
  environment: EnvironmentRef;
  environmentName?: string;
  onClick?: () => void;
}

export const ScopeBadge = memo(function ScopeBadge({
  scope,
  projectPath,
  environment,
  environmentName,
  onClick,
}: ScopeBadgeProps) {
  const { t } = useTranslation();
  const environmentLabel = environmentRefDisplayName(environment, environmentName, t);

  return (
    <Badge
      variant="outline"
      className={`font-normal text-xs ${onClick ? 'cursor-pointer hover:bg-muted' : ''}`}
      onClick={onClick}
    >
      {environmentLabel ? <span className="mr-1">{environmentLabel} ·</span> : null}
      {scope === 'global' ? (
        <>
          <Globe className="w-3 h-3 mr-1" />
          {t('addSkill.scope.global')}
        </>
      ) : (
        <>
          <Folder className="w-3 h-3 mr-1" />
          {projectPathBasename(projectPath ?? '')}
        </>
      )}
    </Badge>
  );
});
