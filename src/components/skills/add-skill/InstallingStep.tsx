// src/components/skills/add-skill/InstallingStep.tsx
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { Progress } from '@/components/ui/progress';
import { installSkills } from '@/hooks/useTauriApi';
import { parseInstallError } from '@/utils/parse-install-error';
import { toAppError } from '@/utils/to-app-error';
import { getCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import { hasFailedMutationUnits } from '@/lib/install-workflow';
import type { WizardState } from './types';
import type { PreparedInstall } from '@/workflows/skill-install-preparation';

/** 安装进度事件（后端 install_skills 中 emit） */
interface InstallProgress {
  phase: 'installing' | 'writing_lock';
  current_skill: string;
  completed: number;
  total: number;
}

type InstallPhase = 'preparing' | 'installing' | 'writing_lock';

interface InstallingStepProps {
  state: WizardState;
  prepared: PreparedInstall;
  updateState: (updates: Partial<WizardState>) => void;
  scope: 'global' | 'project';
  projectPath?: string;
}

export function InstallingStep({ state, prepared, updateState }: InstallingStepProps) {
  const { t } = useTranslation();

  const [phase, setPhase] = useState<InstallPhase>('preparing');
  const [installProgress, setInstallProgress] = useState<InstallProgress | null>(null);

  // 使用 ref 防止重复执行 — advanced-init-once 规则
  const hasStartedRef = useRef(false);

  const updateStateRef = useRef(updateState);
  useEffect(() => { updateStateRef.current = updateState; });

  const tRef = useRef(t);
  useEffect(() => { tRef.current = t; });

  // 捕获当前状态值用于安装
  const installParamsRef = useRef({
    prepared,
    selectedSkills: state.selectedSkills,
    availableSkills: state.availableSkills,
    context: state.context,
  });
  useEffect(() => {
    installParamsRef.current = {
      prepared,
      selectedSkills: state.selectedSkills,
      availableSkills: state.availableSkills,
      context: state.context,
    };
  });

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
      const {
        prepared,
        selectedSkills,
        context,
      } = installParamsRef.current;

      try {
        const results = await installSkills(prepared.request, prepared.preview.token);

        updateStateRef.current({
          installResults: results,
          retrySkillName: undefined,
          retryAgents: undefined,
          retryAgentTargets: undefined,
          step: hasFailedMutationUnits(results) ? 'error' : 'complete',
        });
      } catch (error) {
        console.error('Installation failed:', error);

        const installError = parseInstallError(toAppError(error), tRef.current, {
          selectedSkills,
          availableSkills: installParamsRef.current.availableSkills,
        });
        const guidance = getCrossStorageFailureGuidance(context, 'install', tRef.current);
        if (guidance && !installError.suggestions?.includes(guidance)) {
          installError.suggestions = [...(installError.suggestions ?? []), guidance];
        }

        updateStateRef.current({
          installResults: {
            units: [],
            warnings: [],
          },
          retrySkillName: undefined,
          retryAgents: undefined,
          retryAgentTargets: undefined,
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
        <h3 className="text-lg font-heading font-bold">{t('addSkill.installing.title')}</h3>
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
