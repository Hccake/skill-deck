import { useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Globe, Folder } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useContextStore } from '@/stores/context';
import type { SkillScope } from '@/bindings';

/** 从完整路径中提取项目名称 */
function getProjectName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] || path;
}

interface ScopeStepProps {
  defaultScope?: SkillScope;
  defaultProjectPath?: string;
  onSelect: (scope: SkillScope, projectPath?: string) => void;
}

export function ScopeStep({ defaultScope, defaultProjectPath, onSelect }: ScopeStepProps) {
  const { t } = useTranslation();
  const projects = useContextStore((s) => s.projects);
  const [selected, setSelected] = useState<{ scope: SkillScope; projectPath?: string }>({
    scope: defaultScope ?? 'global',
    projectPath: defaultProjectPath,
  });

  const options = useMemo(() => {
    const items: Array<{ scope: SkillScope; projectPath?: string; label: string; hint: string; icon: typeof Globe }> = [
      {
        scope: 'global',
        label: t('addSkill.scopeSelect.global'),
        hint: t('addSkill.scopeSelect.globalHint'),
        icon: Globe,
      },
      ...projects.map((path) => ({
        scope: 'project' as SkillScope,
        projectPath: path,
        label: getProjectName(path),
        hint: path,
        icon: Folder,
      })),
    ];
    return items;
  }, [projects, t]);

  return (
    <div className="space-y-4 py-4">
      <div className="space-y-2">
        <label className="text-sm font-medium">
          {t('addSkill.scopeSelect.title')}
        </label>
        <p className="text-sm text-muted-foreground">
          {t('addSkill.scopeSelect.hint')}
        </p>
      </div>

      <div className="space-y-2">
        {options.map((option) => {
          const key = option.projectPath || 'global';
          const isSelected = selected.scope === option.scope && selected.projectPath === option.projectPath;
          const Icon = option.icon;

          return (
            <button
              key={key}
              type="button"
              className={`w-full flex items-center gap-3 p-3 rounded-lg border text-left transition-colors cursor-pointer ${
                isSelected
                  ? 'border-primary bg-primary/5'
                  : 'border-border hover:border-primary/50'
              }`}
              onClick={() => setSelected({ scope: option.scope, projectPath: option.projectPath })}
            >
              <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">{option.label}</div>
                <div className="text-xs text-muted-foreground truncate">{option.hint}</div>
              </div>
            </button>
          );
        })}
      </div>

      <div className="flex justify-end pt-2">
        <Button onClick={() => onSelect(selected.scope, selected.projectPath)}>
          {t('addSkill.actions.next')}
        </Button>
      </div>
    </div>
  );
}
