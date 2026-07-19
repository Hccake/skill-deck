// src/pages/WizardPage.tsx
import { useState, useCallback, useEffect, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useProjectStore } from '@/stores/projects';
import { emit } from '@tauri-apps/api/event';
import { Button } from '@/components/ui/button';
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
import { parseWizardContext } from '@/components/skills/add-skill/wizard-context';
import { globalContext } from '@/lib/context';
import { canProceedForStep, getStepFlow } from '@/components/skills/add-skill/types';
import { useMutationMonitor } from '@/hooks/useMutationMonitor';
import { useMutationStore } from '@/stores/mutation';
import { useWindowLifecycle } from '@/lifecycle/useWindowLifecycle';
import type {
  EntryPoint,
  CoreStep,
  WizardStep,
  WizardState,
} from '@/components/skills/add-skill/types';
import type { ContextRef } from '@/bindings';

function createInitialState(params: {
  entryPoint: EntryPoint;
  scope: 'global' | 'project';
  projectPath?: string;
  context: ContextRef;
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
    context: params.context,
    source,
    fetchStatus: 'idle',
    fetchError: null,
    gitRef: null,
    discoverySession: undefined,
    riskPolicy: null,
    riskAcknowledged: false,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: [],
    privateCopyAgents: [],
    allAgents: [],
    availableAgentTargets: [],
    selectedAgentTargets: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    privateCopyAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: false,
    acquiredPayloads: undefined,
    installRequest: undefined,
    installPreview: undefined,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    retrySkillName: undefined,
    retryAgents: undefined,
    retryAgentTargets: undefined,
  };
}

export function WizardPage() {
  const { t } = useTranslation();
  const [searchParams] = useSearchParams();
  const writeBlocked = useMutationStore((store) => store.activeMutation !== null);
  const { requestAction } = useWindowLifecycle();

  useMutationMonitor();

  // 从 URL query 解析参数
  const wizardParams = useMemo(() => {
    const context = parseWizardContext(searchParams.get('context'))
      ?? globalContext({ kind: 'host' });
    return {
      entryPoint: (searchParams.get('entryPoint') ?? 'skills-panel') as EntryPoint,
      scope: context.scope.scope,
      projectPath: searchParams.get('projectPath') ?? undefined,
      context,
      prefillSource: searchParams.get('prefillSource') ?? undefined,
      prefillSkillName: searchParams.get('prefillSkillName') ?? undefined,
    };
  }, [searchParams]);

  const refreshProjects = useProjectStore((store) => store.refresh);
  useEffect(() => {
    if (wizardParams.entryPoint === 'discovery') {
      void refreshProjects(wizardParams.context.environment);
    }
  }, [refreshProjects, wizardParams.context.environment, wizardParams.entryPoint]);

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

  const closeWizard = useCallback(
    () => requestAction('closeCurrentWindow'),
    [requestAction],
  );

  // 重试安装 — 清除错误状态，递增 key 强制 InstallingStep 重新挂载
  const handleRetryInstall = useCallback(() => {
    updateState({
      installResults: null,
      installError: undefined,
      retrySkillName: undefined,
      retryAgents: undefined,
      retryAgentTargets: undefined,
      step: 'installing',
    });
    setInstallKey((k) => k + 1);
  }, [updateState]);

  // 完成安装 — 通知主窗口刷新 skills 列表，然后关闭窗口
  const handleDone = useCallback(async () => {
    try { await emit('wizard-result', { action: 'refresh' }); } catch { /* ignore */ }
    await closeWizard();
  }, [closeWizard]);

  // 验证是否可以进入下一步
  const canProceed = useMemo(() => canProceedForStep(state), [state]);

  // 是否为结果态
  const isResultState = state.step === 'installing' || state.step === 'complete' || state.step === 'error';
  // 是否显示 Scope badge（从 step 2 Source 开始显示）
  const showScopeBadge = currentStepIndex >= 1 || isResultState;
  const hasOverwrites = useMemo(
    () => Object.values(state.overwrites).some((agents) => agents.length > 0),
    [state.overwrites]
  );

  const handleStepClick = useCallback((step: CoreStep) => {
    const clickedIndex = steps.indexOf(step);
    if (clickedIndex < currentStepIndex) {
      goToStep(step);
    }
  }, [steps, currentStepIndex, goToStep]);

  const handleScopeBadgeClick = useMemo(
    () => currentStepIndex > 0 ? () => goToStep(steps[0]) : undefined,
    [currentStepIndex, goToStep, steps]
  );

  // 渲染当前步骤内容
  const renderContent = () => {
    switch (state.step) {
      case 'scope':
        return (
          <ScopeStep
            state={state}
            updateState={updateState}
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
            onRetry={() => {
              updateState({
                installResults: null,
                installRequest: undefined,
                installPreview: undefined,
                confirmReady: false,
              });
              goToStep('confirm');
            }}
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
            onRetry={() => {
              updateState({
                installResults: null,
                installRequest: undefined,
                installPreview: undefined,
                confirmReady: false,
              });
              goToStep('confirm');
            }}
          />
        );
      default:
        return null;
    }
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      {/* 左侧向导栏 (Sidebar) */}
      <div className="w-52 flex-shrink-0 bg-muted/10 border-r flex flex-col relative z-10">
        <div className="px-6 pt-8 pb-4">
          <h1 className="text-xl font-bold tracking-tight">{t('addSkill.title')}</h1>
        </div>

        <div className="flex-1 overflow-y-auto px-2 pb-4">
          {!isResultState && (
            <StepIndicator
              entryPoint={state.entryPoint}
              currentStep={state.step}
              orientation="vertical"
              onStepClick={handleStepClick}
            />
          )}
        </div>

        {/* 底部 Scope 徽章区域 */}
        <div className="px-4 h-[72px] mt-auto border-t bg-muted/5 flex items-center justify-center">
          {showScopeBadge && (
              <ScopeBadge
                scope={state.scope}
                projectPath={state.projectPath}
                onClick={handleScopeBadgeClick}
              />
          )}
        </div>
      </div>

      {/* 右侧主内容区 (Main Content) */}
      <div className="flex-1 flex flex-col min-w-0 bg-background relative">
        {/* 内容滚动区 */}
        <div className="flex-1 min-h-0 overflow-y-auto p-8">
          <div key={state.step} className="h-full animate-in fade-in slide-in-from-bottom-2 duration-500">
            {renderContent()}
          </div>
        </div>

        {/* 底部操作栏 */}
        {!isResultState && (
          <div className="flex-shrink-0 h-[72px] border-t bg-background/80 backdrop-blur-sm px-8 flex items-center justify-end gap-3 z-10 shadow-[0_-4px_12px_rgba(0,0,0,0.02)]">
            <Button variant="outline" onClick={closeWizard}>
              {t('addSkill.actions.cancel')}
            </Button>
            {currentStepIndex > 0 && (
              <Button variant="outline" onClick={goBack}>
                {t('addSkill.actions.back')}
              </Button>
            )}
            {state.step === 'confirm' ? (
              <Button
                onClick={() => goToStep('installing')}
                disabled={!canProceed || writeBlocked}
                className="min-w-[100px]"
              >
                {hasOverwrites ? (
                  t('addSkill.actions.installWithOverwrite')
                ) : (
                  t('addSkill.actions.install')
                )}
              </Button>
            ) : (
              <Button onClick={goNext} disabled={!canProceed} className="min-w-[100px]">
                {t('addSkill.actions.next')}
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
