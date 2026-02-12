// src/components/skills/add-skill/ScopeBadge.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Globe, Folder } from 'lucide-react';
import { Badge } from '@/components/ui/badge';

interface ScopeBadgeProps {
  scope: 'global' | 'project';
  projectPath?: string;
  onClick?: () => void;
}

/** 从路径提取项目名 */
function getProjectName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] || path;
}

export const ScopeBadge = memo(function ScopeBadge({
  scope,
  projectPath,
  onClick,
}: ScopeBadgeProps) {
  const { t } = useTranslation();

  return (
    <Badge
      variant="outline"
      className={`font-normal text-xs ${onClick ? 'cursor-pointer hover:bg-muted' : ''}`}
      onClick={onClick}
    >
      {scope === 'global' ? (
        <>
          <Globe className="w-3 h-3 mr-1" />
          {t('addSkill.scope.global')}
        </>
      ) : (
        <>
          <Folder className="w-3 h-3 mr-1" />
          {getProjectName(projectPath ?? '')}
        </>
      )}
    </Badge>
  );
});
