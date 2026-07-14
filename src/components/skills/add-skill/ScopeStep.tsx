import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Globe, Folder, Check } from 'lucide-react';
import { useProjectStore } from '@/stores/projects';
import { getSharedSkillDirectory } from '@/lib/agentTargets';
import { environmentKey } from '@/lib/context';
import type { ContextRef, SkillScope } from '@/bindings';
import type { WizardState } from './types';

const EMPTY_PROJECTS: ReturnType<typeof useProjectStore.getState>['projectsByEnvironment'][string] = [];

type ScopeOption = {
  scope: SkillScope;
  projectPath?: string;
  context: ContextRef;
  label: string;
  hint: string;
  icon: typeof Globe;
};

/** 从完整路径中提取项目名称 */
function getProjectName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] || path;
}

interface ScopeStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState>) => void;
}

export function ScopeStep({ state, updateState }: ScopeStepProps) {
  const { t } = useTranslation();
  const environment = state.context.environment;
  const environmentProjects = useProjectStore((store) => (
    store.projectsByEnvironment[environmentKey(environment)] ?? EMPTY_PROJECTS
  ));

  const globalOption: ScopeOption = {
    scope: 'global' as SkillScope,
    label: t('addSkill.scopeSelect.global'),
    hint: environment.kind === 'host'
      ? t('addSkill.scopeSelect.globalHint', { path: getSharedSkillDirectory('global') })
      : t('addSkill.scopeSelect.environmentGlobalHint'),
    icon: Globe,
    context: { environment, scope: { scope: 'global' } },
  };

  const projectOptions = useMemo<ScopeOption[]>(() => {
    return environmentProjects.map(({ binding: project }) => ({
      scope: 'project' as SkillScope,
      projectPath: project.nativePath,
      label: project.displayName ?? getProjectName(project.nativePath),
      hint: project.nativePath,
      icon: Folder,
      context: {
        environment,
        scope: { scope: 'project', project_id: project.id },
      },
    }));
  }, [environment, environmentProjects]);

  const renderRow = (option: ScopeOption, isSelected: boolean) => {
    const Icon = option.icon;
    return (
      <button
        key={option.projectPath || 'global'}
        type="button"
        className={`w-full flex items-center gap-4 px-4 py-3 transition-colors cursor-pointer text-left relative ${
          isSelected ? 'bg-primary/5' : 'hover:bg-muted/50'
        }`}
        onClick={() => updateState({
          scope: option.scope,
          projectPath: option.projectPath,
          context: option.context,
        })}
      >
        {/* 左侧图标 */}
        <div className={`p-2 rounded-md ${isSelected ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground'}`}>
          <Icon className="h-5 w-5 shrink-0" />
        </div>

        {/* 中间文字 */}
        <div className="min-w-0 flex-1 text-left">
          <div className={`text-sm font-medium transition-colors ${isSelected ? 'text-primary' : 'text-foreground'}`}>
            {option.label}
          </div>
          <div className="text-xs text-muted-foreground truncate mt-0.5">{option.hint}</div>
        </div>

        {/* 右侧 Checkmark */}
        {isSelected && (
          <div className="shrink-0 pl-4 animate-in fade-in zoom-in-50 duration-200">
            <Check className="h-5 w-5 text-primary" />
          </div>
        )}
      </button>
    );
  };

  return (
    <div className="space-y-6">
      <div className="space-y-2">
        <label className="text-base font-semibold">
          {t('addSkill.scopeSelect.title')}
        </label>
        <p className="text-sm text-muted-foreground">
          {t('addSkill.scopeSelect.hint')}
        </p>
      </div>

      <div className="space-y-6">
        {/* 全局安装：独立的组 */}
        <div className="rounded-xl border bg-card shadow-sm overflow-hidden">
          {renderRow(globalOption, state.scope === 'global')}
        </div>

        {/* 项目级安装：带细线分割的列表组 */}
        {projectOptions.length > 0 && (
          <div className="space-y-3">
            <h3 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider pl-1">
              {t('addSkill.scopeSelect.localProjects')}
            </h3>
            <div className="rounded-xl border bg-card shadow-sm overflow-hidden divide-y divide-border">
              {projectOptions.map((option) =>
                renderRow(option, state.scope === 'project' && state.projectPath === option.projectPath)
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
