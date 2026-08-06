import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Globe, Folder } from 'lucide-react';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import { getSharedSkillDirectory } from '@/lib/agentTargets';
import type { ContextRef, SkillScope } from '@/bindings';
import type { WizardState } from './types';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { projectBindingDisplayName } from '@/lib/projects/presentation';

type ScopeOption = {
  scope: SkillScope;
  projectPath?: string;
  context: ContextRef;
  label: string;
  hint: string;
  icon: typeof Globe;
};

interface ScopeStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState>) => void;
}

export function ScopeStep({ state, updateState }: ScopeStepProps) {
  const { t } = useTranslation();
  const environment = state.context.environment;
  const { projects: environmentProjects } = useProjectWorkspace(environment);

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
      label: projectBindingDisplayName(project),
      hint: project.nativePath,
      icon: Folder,
      context: {
        environment,
        scope: { scope: 'project', project_id: project.id },
      },
    }));
  }, [environment, environmentProjects]);

  const options = [globalOption, ...projectOptions];
  const selectedValue = state.scope === 'global'
    ? 'global'
    : `project:${state.context.scope.scope === 'project' ? state.context.scope.project_id : ''}`;

  const renderRow = (option: ScopeOption, isSelected: boolean) => {
    const Icon = option.icon;
    const value = option.context.scope.scope === 'global'
      ? 'global'
      : `project:${option.context.scope.project_id}`;
    const id = `scope-${value.replace(/[^a-zA-Z0-9_-]/g, '-')}`;
    return (
      <Label
        key={value}
        htmlFor={id}
        className={`w-full flex items-center gap-4 px-4 py-3 transition-colors cursor-pointer text-left relative ${
          isSelected ? 'bg-primary/5' : 'hover:bg-muted/50'
        }`}
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

        <RadioGroupItem id={id} value={value} className="ml-4" />
      </Label>
    );
  };

  const handleValueChange = (value: string) => {
    const option = options.find((candidate) => (
      candidate.context.scope.scope === 'global'
        ? value === 'global'
        : value === `project:${candidate.context.scope.project_id}`
    ));
    if (!option) return;
    updateState({
      scope: option.scope,
      projectPath: option.projectPath,
      context: option.context,
    });
  };

  return (
    <div className="space-y-6">
      <div className="space-y-2">
        <h2 id="scope-step-title" className="text-base font-semibold">
          {t('addSkill.scopeSelect.title')}
        </h2>
        <p className="text-sm text-muted-foreground">
          {t('addSkill.scopeSelect.hint')}
        </p>
      </div>

      <RadioGroup
        value={selectedValue}
        onValueChange={handleValueChange}
        aria-labelledby="scope-step-title"
        className="space-y-6"
      >
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
      </RadioGroup>
    </div>
  );
}
