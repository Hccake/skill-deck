// src/pages/WizardPage.tsx
import { useState, useCallback, useEffect, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useContextStore } from '@/stores/context';
import { emit } from '@tauri-apps/api/event';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { StepIndicator } from '@/components/skills/add-skill/StepIndicator';
import { ScopeBadge } from '@/components/skills/add-skill/ScopeBadge';
import { ScopeStep } from '@/components/skills/add-skill/ScopeStep';
import { SourceStep } from '@/components/skills/add-skill/SourceStep';
import { SkillsStep } from '@/components/skills/add-skill/SkillsStep';
import { OptionsStep } from '@/components/skills/add-skill/OptionsStep';
import { ConfirmStep } from '@/components/skills/add-skill/ConfirmStep';
import { InstallingStep } from '@/components/skills/add-skill/InstallingStep';
import { CompleteStep } from '@/components/skills/add-skill/CompleteStep';
import { ErrorStep } from '@/components/skills/add-skill/ErrorStep';
import { getStepFlow } from '@/components/skills/add-skill/types';
import type {
  EntryPoint,
  CoreStep,
  WizardStep,
  WizardState,
} from '@/components/skills/add-skill/types';

function createInitialState(params: {
  entryPoint: EntryPoint;
  scope: 'global' | 'project';
  projectPath?: string;
  prefillSource?: string;
  prefillSkillName?: string;
}): WizardState {
  const steps = getStepFlow(params.entryPoint);

  // Discovery 入口：拼接 source@skillName 格式，让 SourceStep 的 @skill 语法预选逻辑自动生效
  let source = params.prefillSource ?? '';
  if (source && params.prefillSkillName) {
    source = `${source}@${params.prefillSkillName}`;
  }

  return {
    step: steps[0],
    entryPoint: params.entryPoint,
    scope: params.scope,
    projectPath: params.projectPath,
    source,
    fetchStatus: 'idle',
    fetchError: null,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: [],
    allAgents: [],
    agentsCollapsed: false,
    mode: 'symlink',
    otherAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
  };
}

export function WizardPage() {
  const { t } = useTranslation();
  const [searchParams] = useSearchParams();

  // 从 URL query 解析参数
  const wizardParams = useMemo(() => ({
    entryPoint: (searchParams.get('entryPoint') ?? 'skills-panel') as EntryPoint,
    scope: (searchParams.get('scope') ?? 'global') as 'global' | 'project',
    projectPath: searchParams.get('projectPath') ?? undefined,
    prefillSource: searchParams.get('prefillSource') ?? undefined,
    prefillSkillName: searchParams.get('prefillSkillName') ?? undefined,
  }), [searchParams]);

  // Discovery 入口需要 ScopeStep，确保子窗口中 projects 列表已加载
  const { projectsLoaded, loadProjects } = useContextStore();
  useEffect(() => {
    if (!projectsLoaded) {
      loadProjects();
    }
  }, [projectsLoaded, loadProjects]);

  const [state, setState] = useState<WizardState>(() =>
    createInitialState(wizardParams)
  );

  // 用于强制 InstallingStep 重新挂载（重试安装时递增）
  const [installKey, setInstallKey] = useState(0);

  const updateState = useCallback(
    (updates: Partial<WizardState> | ((prev: WizardState) => Partial<WizardState>)) => {
      setState((prev) => ({
        ...prev,
        ...(typeof updates === 'function' ? updates(prev) : updates),
      }));
    },
    []
  );

  // 步骤流程
  const steps = useMemo(() => getStepFlow(state.entryPoint), [state.entryPoint]);

  const currentStepIndex = steps.indexOf(state.step as CoreStep);

  const goToStep = useCallback((step: WizardStep) => {
    updateState({ step });
  }, [updateState]);

  const goNext = useCallback(() => {
    if (currentStepIndex >= 0 && currentStepIndex < steps.length - 1) {
      goToStep(steps[currentStepIndex + 1]);
    }
  }, [currentStepIndex, steps, goToStep]);

  const goBack = useCallback(() => {
    if (currentStepIndex > 0) {
      goToStep(steps[currentStepIndex - 1]);
    }
  }, [currentStepIndex, steps, goToStep]);

  // 关闭窗口
  const closeWizard = useCallback(async () => {
    await getCurrentWebviewWindow().close();
  }, []);

  // 重试安装 — 清除错误状态，递增 key 强制 InstallingStep 重新挂载
  const handleRetryInstall = useCallback(() => {
    updateState({ installResults: null, installError: undefined, step: 'installing' });
    setInstallKey((k) => k + 1);
  }, [updateState]);

  // 完成安装 — 通知主窗口刷新 skills 列表，然后关闭窗口
  const handleDone = useCallback(async () => {
    try { await emit('wizard-result', { action: 'refresh' }); } catch { /* ignore */ }
    await closeWizard();
  }, [closeWizard]);

  // 验证是否可以进入下一步
  const canProceed = useMemo(() => {
    switch (state.step) {
      case 'source':
        return state.fetchStatus === 'success' && state.availableSkills.length > 0;
      case 'scope':
        return true;
      case 'skills':
        return state.selectedSkills.length > 0;
      case 'options':
        return true;
      case 'confirm':
        return true;
      default:
        return false;
    }
  }, [state.step, state.fetchStatus, state.availableSkills.length, state.selectedSkills.length]);

  // 是否为结果态
  const isResultState = state.step === 'installing' || state.step === 'complete' || state.step === 'error';
  // 是否显示 Scope badge（从 step 2 Source 开始显示）
  const showScopeBadge = currentStepIndex >= 1 || isResultState;

  // 渲染当前步骤内容
  const renderContent = () => {
    switch (state.step) {
      case 'scope':
        return (
          <ScopeStep
            defaultScope={state.scope}
            defaultProjectPath={state.projectPath}
            onSelect={(selectedScope, selectedProjectPath) => {
              updateState({
                scope: selectedScope,
                projectPath: selectedProjectPath,
              });
              goNext();
            }}
          />
        );
      case 'source':
        return (
          <SourceStep
            state={state}
            updateState={updateState}
            onNext={goNext}
            autoFetch={!!wizardParams.prefillSource}
          />
        );
      case 'skills':
        return <SkillsStep state={state} updateState={updateState} />;
      case 'options':
        return <OptionsStep state={state} updateState={updateState} />;
      case 'confirm':
        return (
          <ConfirmStep
            state={state}
            updateState={updateState}
            scope={state.scope}
            projectPath={state.projectPath}
          />
        );
      case 'installing':
        return (
          <InstallingStep
            key={installKey}
            state={state}
            updateState={updateState}
            scope={state.scope}
            projectPath={state.projectPath}
          />
        );
      case 'complete':
        return (
          <CompleteStep
            state={state}
            onDone={handleDone}
            onRetry={() => goToStep('confirm')}
          />
        );
      case 'error':
        if (state.installError) {
          return (
            <ErrorStep
              error={state.installError}
              onRetry={handleRetryInstall}
              onBack={() => goToStep(steps[0])}
              onClose={closeWizard}
            />
          );
        }
        return (
          <CompleteStep
            state={state}
            onDone={handleDone}
            onRetry={() => goToStep('confirm')}
          />
        );
      default:
        return null;
    }
  };

  return (
    <div className="flex flex-col h-screen bg-background text-foreground">
      {/* 标题 + 步骤指示器 — 固定顶部 */}
      <div className="flex-shrink-0">
        <div className="flex items-center justify-between px-5 pt-4 pb-2">
          <h1 className="text-lg font-semibold">{t('addSkill.title')}</h1>
          {showScopeBadge && (
            <ScopeBadge
              scope={state.scope}
              projectPath={state.projectPath}
              onClick={currentStepIndex > 0 ? () => goToStep(steps[0]) : undefined}
            />
          )}
        </div>
        {!isResultState && (
          <StepIndicator
            entryPoint={state.entryPoint}
            currentStep={state.step}
            onStepClick={(step) => {
              const clickedIndex = steps.indexOf(step);
              if (clickedIndex < currentStepIndex) {
                goToStep(step);
              }
            }}
          />
        )}
        <Separator />
      </div>

      {/* 内容区 — 弹性高度，独立滚动 */}
      <div className="flex-1 min-h-0 overflow-y-auto px-5 py-4">
        {renderContent()}
      </div>

      {/* 底部操作栏 — 固定底部 */}
      {!isResultState && state.step !== 'scope' && (
        <>
          <Separator />
          <div className="flex-shrink-0 flex justify-end gap-2 px-5 py-3">
            <Button variant="outline" onClick={closeWizard}>
              {t('addSkill.actions.cancel')}
            </Button>
            {currentStepIndex > 0 && (
              <Button variant="outline" onClick={goBack}>
                {t('addSkill.actions.back')}
              </Button>
            )}
            {state.step === 'confirm' ? (
              <Button onClick={() => goToStep('installing')}>
                {t('addSkill.actions.install')}
              </Button>
            ) : (
              <Button onClick={goNext} disabled={!canProceed}>
                {t('addSkill.actions.next')}
              </Button>
            )}
          </div>
        </>
      )}
    </div>
  );
}
