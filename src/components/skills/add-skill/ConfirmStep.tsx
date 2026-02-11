// src/components/skills/add-skill/ConfirmStep.tsx
import { useEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Globe, Folder } from 'lucide-react';
import { checkOverwrites } from '@/hooks/useTauriApi';
import type { AddSkillState } from './types';

/** 从完整路径中提取项目名称 */
function getProjectName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] || path;
}

interface ConfirmStepProps {
  state: AddSkillState;
  updateState: (updates: Partial<AddSkillState>) => void;
  scope: 'global' | 'project';
  projectPath?: string;
}

export function ConfirmStep({ state, updateState, scope, projectPath }: ConfirmStepProps) {
  const { t } = useTranslation();

  const updateStateRef = useRef(updateState);
  updateStateRef.current = updateState;

  // 获取选中的 agent 名称 — rerender-memo 规则
  const selectedAgentNames = useMemo(() => {
    const selectedSet = new Set(state.selectedAgents);
    return state.allAgents
      .filter((a) => selectedSet.has(a.id))
      .map((a) => a.displayName);
  }, [state.selectedAgents, state.allAgents]);

  // 检测覆盖
  useEffect(() => {
    async function detectOverwrites() {
      try {
        const overwrites = await checkOverwrites(
          state.selectedSkills,
          state.selectedAgents,
          scope,
          scope === 'project' ? projectPath : undefined
        );
        updateStateRef.current({ overwrites });
      } catch (error) {
        console.error('Failed to check overwrites:', error);
        updateStateRef.current({ overwrites: {} });
      }
    }

    if (state.selectedAgents.length > 0 && state.selectedSkills.length > 0) {
      detectOverwrites();
    }
  }, [state.selectedSkills, state.selectedAgents, scope, projectPath]);

  const totalAgents = state.selectedAgents.length;
  const hasOverwrites = Object.keys(state.overwrites).length > 0;

  return (
    <div className="space-y-4 py-4">
      <h3 className="text-sm font-medium">{t('addSkill.confirm.title')}</h3>

      {/* Scope 信息 Banner */}
      <div className="flex items-center gap-2 p-3 bg-muted/50 text-foreground text-sm rounded-md border border-border/50">
        {scope === 'global' ? (
          <>
            <Globe className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span>{t('addSkill.confirm.scopeGlobal')}</span>
          </>
        ) : (
          <>
            <Folder className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span>
              {t('addSkill.confirm.scopeProject', {
                name: getProjectName(projectPath ?? ''),
                path: projectPath,
              })}
            </span>
          </>
        )}
      </div>

      <div className="border rounded-md p-3 space-y-3">
          {state.selectedSkills.map((skillName) => {
            const overwriteAgents = state.overwrites[skillName] ?? [];

            return (
              <div key={skillName} className="p-3 bg-muted/30 rounded-md space-y-2">
                <div className="font-medium text-sm">
                  {scope === 'global' ? '~/.agents/skills/' : './.agents/skills/'}
                  {skillName}
                </div>

                {selectedAgentNames.length > 0 && (
                  <div className="text-xs text-muted-foreground">
                    <span>
                      {state.mode === 'symlink'
                        ? t('addSkill.confirm.symlink')
                        : t('addSkill.confirm.copy')}
                    </span>{' '}
                    {selectedAgentNames.join(', ')}
                  </div>
                )}

                {overwriteAgents.length > 0 && (
                  <div className="flex items-center gap-2 text-xs text-amber-600 dark:text-amber-500">
                    <AlertTriangle className="h-3 w-3" />
                    <span>
                      {t('addSkill.confirm.overwrites')}: {overwriteAgents.join(', ')}
                    </span>
                  </div>
                )}
              </div>
            );
          })}
      </div>

      {/* Summary */}
      <div className="text-sm text-muted-foreground">
        {t('addSkill.confirm.installing', {
          count: state.selectedSkills.length,
          agents: totalAgents,
        })}
      </div>

      {/* Overwrite warning banner */}
      {hasOverwrites && (
        <div className="flex items-center gap-2 p-3 bg-amber-500/10 text-amber-700 dark:text-amber-400 text-sm rounded-md">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          {t('addSkill.confirm.overwriteWarning')}
        </div>
      )}
    </div>
  );
}
