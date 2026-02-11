// src/components/skills/add-skill/InstallingStep.tsx
import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Progress } from '@/components/ui/progress';
import { installSkills, saveLastSelectedAgents } from '@/hooks/useTauriApi';
import type { AddSkillState, InstallParams } from './types';

interface InstallingStepProps {
  state: AddSkillState;
  updateState: (updates: Partial<AddSkillState>) => void;
  scope: 'global' | 'project';
  projectPath?: string;
}

export function InstallingStep({ state, updateState, scope, projectPath }: InstallingStepProps) {
  const { t } = useTranslation();

  // 使用 ref 防止重复执行 — advanced-init-once 规则
  const hasStartedRef = useRef(false);

  const updateStateRef = useRef(updateState);
  updateStateRef.current = updateState;

  // 捕获当前状态值用于安装
  const installParamsRef = useRef({
    source: state.source,
    selectedSkills: state.selectedSkills,
    selectedAgents: state.selectedAgents,
    mode: state.mode,
    scope,
    projectPath,
  });
  installParamsRef.current = {
    source: state.source,
    selectedSkills: state.selectedSkills,
    selectedAgents: state.selectedAgents,
    mode: state.mode,
    scope,
    projectPath,
  };

  // 执行安装 - 只运行一次
  useEffect(() => {
    if (hasStartedRef.current) return;
    hasStartedRef.current = true;

    async function doInstall() {
      const { source, selectedSkills, selectedAgents, mode, scope: installScope, projectPath: installProjectPath } = installParamsRef.current;

      const params: InstallParams = {
        source,
        skills: selectedSkills,
        agents: selectedAgents,
        scope: installScope,
        projectPath: installScope === 'project' ? installProjectPath : undefined,
        mode,
      };

      try {
        const results = await installSkills(params);

        // 安装成功后静默保存选择的 agents
        if (results.failed.length === 0) {
          try {
            await saveLastSelectedAgents(selectedAgents);
          } catch (error) {
            console.error('Failed to save selected agents:', error);
          }
        }

        updateStateRef.current({
          installResults: results,
          step: results.failed.length > 0 ? 'error' : 'complete',
        });
      } catch (error) {
        console.error('Installation failed:', error);
        updateStateRef.current({
          installResults: {
            successful: [],
            failed: [],
            symlinkFallbackAgents: [],
          },
          step: 'error',
        });
      }
    }

    doInstall();
  }, []);

  const progress = state.installResults ? 100 : 50;

  return (
    <div className="flex flex-col items-center justify-center py-12 space-y-6">
      <div className="w-12 h-12 border-4 border-primary border-t-transparent rounded-full animate-spin" />

      <div className="text-center space-y-2">
        <h3 className="text-lg font-medium">{t('addSkill.installing.title')}</h3>
        <p className="text-sm text-muted-foreground">
          {t('addSkill.installing.progress', {
            completed: state.installResults ? state.selectedSkills.length : 0,
            total: state.selectedSkills.length,
          })}
        </p>
      </div>

      <div className="w-full max-w-xs">
        <Progress value={progress} />
      </div>
    </div>
  );
}
