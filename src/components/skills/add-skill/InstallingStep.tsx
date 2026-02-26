// src/components/skills/add-skill/InstallingStep.tsx
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { Progress } from '@/components/ui/progress';
import { installSkills, saveLastSelectedAgents } from '@/hooks/useTauriApi';
import { parseInstallError } from '@/utils/parse-install-error';
import { toAppError } from '@/utils/to-app-error';
import type { InstallParams } from '@/bindings';
import type { WizardState } from './types';

/** 克隆进度事件（与 SourceStep 共用后端事件） */
interface CloneProgress {
  phase: 'connecting' | 'cloning' | 'done' | 'error';
  elapsed_secs: number;
  timeout_secs: number;
  message: string | null;
}

/** 安装进度事件（后端 install_skills 中 emit） */
interface InstallProgress {
  phase: 'installing' | 'writing_lock';
  current_skill: string;
  completed: number;
  total: number;
}

type InstallPhase = 'preparing' | 'cloning' | 'installing' | 'writing_lock';

interface InstallingStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState>) => void;
  scope: 'global' | 'project';
  projectPath?: string;
}

export function InstallingStep({ state, updateState, scope, projectPath }: InstallingStepProps) {
  const { t } = useTranslation();

  const [phase, setPhase] = useState<InstallPhase>('preparing');
  const [cloneProgress, setCloneProgress] = useState<CloneProgress | null>(null);
  const [installProgress, setInstallProgress] = useState<InstallProgress | null>(null);

  // 使用 ref 防止重复执行 — advanced-init-once 规则
  const hasStartedRef = useRef(false);

  const updateStateRef = useRef(updateState);
  useEffect(() => { updateStateRef.current = updateState; });

  // 捕获当前状态值用于安装
  const installParamsRef = useRef({
    source: state.source,
    selectedSkills: state.selectedSkills,
    selectedAgents: state.selectedAgents,
    mode: state.mode,
    scope,
    projectPath,
  });
  useEffect(() => {
    installParamsRef.current = {
      source: state.source,
      selectedSkills: state.selectedSkills,
      selectedAgents: state.selectedAgents,
      mode: state.mode,
      scope,
      projectPath,
    };
  });

  // 监听克隆进度事件
  useEffect(() => {
    const unlisten = listen<CloneProgress>('clone-progress', (event) => {
      setCloneProgress(event.payload);
      if (event.payload.phase !== 'done' && event.payload.phase !== 'error') {
        setPhase('cloning');
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // 监听安装进度事件
  useEffect(() => {
    const unlisten = listen<InstallProgress>('install-progress', (event) => {
      setInstallProgress(event.payload);
      setPhase(event.payload.phase as InstallPhase);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

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
        projectPath: installScope === 'project' ? (installProjectPath ?? null) : null,
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

        const installError = parseInstallError(toAppError(error), t, {
          selectedSkills,
          availableSkills: state.availableSkills,
        });

        updateStateRef.current({
          installResults: {
            successful: [],
            failed: [],
            symlinkFallbackAgents: [],
          },
          installError,
          step: 'error',
        });
      }
    }

    doInstall();
  }, []);

  // 阶段文字
  const getPhaseText = () => {
    switch (phase) {
      case 'preparing':
        return t('addSkill.installing.preparing');
      case 'cloning':
        if (cloneProgress?.phase === 'connecting') {
          return t('addSkill.installing.connecting');
        }
        if (cloneProgress?.phase === 'cloning') {
          return t('addSkill.installing.cloningWithTime', {
            elapsed: cloneProgress.elapsed_secs,
            timeout: cloneProgress.timeout_secs,
          });
        }
        return t('addSkill.installing.cloning');
      case 'installing':
        if (installProgress) {
          return t('addSkill.installing.installingSkill', {
            skill: installProgress.current_skill,
            completed: installProgress.completed + 1,
            total: installProgress.total,
          });
        }
        return t('addSkill.installing.title');
      case 'writing_lock':
        return t('addSkill.installing.writingConfig');
      default:
        return t('addSkill.installing.title');
    }
  };

  // 智能进度条：clone 阶段用确定值，其他阶段用 indeterminate
  const getProgressValue = (): number | undefined => {
    if (phase === 'cloning' && cloneProgress?.phase === 'cloning') {
      return Math.min(
        (cloneProgress.elapsed_secs / cloneProgress.timeout_secs) * 100,
        99,
      );
    }
    if (phase === 'installing' && installProgress) {
      return Math.min(
        (installProgress.completed / installProgress.total) * 100,
        99,
      );
    }
    // preparing / writing_lock / other: indeterminate
    return undefined;
  };

  const progressValue = getProgressValue();
  const isIndeterminate = progressValue === undefined;

  return (
    <div className="flex flex-col items-center justify-center py-12 space-y-6">
      <div className="w-10 h-10 border-3 border-primary border-t-transparent rounded-full animate-spin" />

      <div className="text-center space-y-1.5">
        <h3 className="text-lg font-medium">{t('addSkill.installing.title')}</h3>
        <p className="text-sm text-muted-foreground">{getPhaseText()}</p>
      </div>

      <div className="w-full max-w-xs">
        {isIndeterminate ? (
          <div className="h-2 w-full bg-secondary rounded-full overflow-hidden">
            <div className="h-full w-1/3 bg-primary rounded-full animate-indeterminate" />
          </div>
        ) : (
          <Progress value={progressValue} className="h-2" />
        )}
      </div>
    </div>
  );
}
