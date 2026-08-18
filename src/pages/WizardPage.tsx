// src/pages/WizardPage.tsx
import { useState, useCallback, useEffect, useMemo, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { RotateCw } from 'lucide-react';
import { emit } from '@tauri-apps/api/event';
import { toast } from 'sonner';
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
import {
  confirmInstallAgentSelection,
  getInstallAgentSelection,
} from '@/hooks/useTauriApi';
import {
  useAgentSelectionSession,
  type InstallAgentSelectionSessionRequest,
} from '@/hooks/useAgentSelectionSession';
import { isRetryableMutationUnit } from '@/lib/mutation-results';
import type {
  EntryPoint,
  CoreStep,
  WizardStep,
  WizardState,
} from '@/components/skills/add-skill/types';
import type { SkillLocationRef } from '@/bindings';
import { cn } from '@/lib/utils';

type InstallResults = NonNullable<WizardState['installResults']>;

function createInitialState(params: {
  entryPoint: EntryPoint;
  scope: 'global' | 'project';
  projectPath?: string;
  context: SkillLocationRef;
  environmentName?: string;
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
    environmentName: params.environmentName,
    sourceInput: source,
    source: '',
    fetchStatus: 'idle',
    fetchError: null,
    gitRef: null,
    discoverySession: undefined,
    redirectedDownloadHost: null,
    redirectAcknowledged: false,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    overwrites: {},
    preparation: { status: 'idle' },
    agentSelectionIntent: { wildcardRequested: false, explicitAgentIds: [] },
    installResults: null,
  };
}

export function WizardPage() {
  const { t } = useTranslation();
  const [searchParams] = useSearchParams();
  const writeBlocked = useMutationStore((store) => store.activeMutation !== null);
  const { requestAction } = useWindowLifecycle();

  useMutationMonitor();

  useEffect(() => {
    document.title = t('addSkill.title');
  }, [t]);

  // 从 URL query 解析参数
  const wizardParams = useMemo(() => {
    const context = parseWizardContext(searchParams.get('context'))
      ?? globalContext({ kind: 'native' });
    return {
      entryPoint: (searchParams.get('entryPoint') ?? 'skills-panel') as EntryPoint,
      scope: context.scope.scope,
      projectPath: searchParams.get('projectPath') ?? undefined,
      context,
      environmentName: searchParams.get('environmentName') ?? undefined,
      prefillSource: searchParams.get('prefillSource') ?? undefined,
      prefillSkillName: searchParams.get('prefillSkillName') ?? undefined,
    };
  }, [searchParams]);

  const [state, setState] = useState<WizardState>(() =>
    createInitialState(wizardParams)
  );
  const notifiedInstallResultsRef = useRef<InstallResults | null>(null);
  const notificationInFlightRef = useRef<{
    results: InstallResults;
    promise: Promise<boolean>;
  } | null>(null);

  // 用于强制 InstallingStep 重新挂载（重试安装时递增）
  const [installKey, setInstallKey] = useState(0);
  const [confirmingAgentSelection, setConfirmingAgentSelection] = useState(false);

  const updateState = useCallback(
    (updates: Partial<WizardState> | ((prev: WizardState) => Partial<WizardState>)) => {
      setState((prev) => ({
        ...prev,
        ...(typeof updates === 'function' ? updates(prev) : updates),
      }));
    },
    []
  );

  const agentSelectionRequest = useMemo<InstallAgentSelectionSessionRequest>(() => ({
    kind: 'install',
    context: state.context,
    intent: state.agentSelectionIntent,
  }), [state.context, state.agentSelectionIntent]);
  const loadAgentSelection = useCallback(
    (request: InstallAgentSelectionSessionRequest) => getInstallAgentSelection(
      request.context,
      request.intent,
    ),
    [],
  );
  const agentSelection = useAgentSelectionSession({
    active: state.step === 'options' || state.step === 'confirm',
    request: agentSelectionRequest,
    load: loadAgentSelection,
  });

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

  const handleOptionsNext = useCallback(async () => {
    if (agentSelection.status !== 'ready') return;
    setConfirmingAgentSelection(true);
    try {
      const outcome = await confirmInstallAgentSelection(
        state.context,
        agentSelection.submission,
        state.agentSelectionIntent,
      );
      if (outcome.status === 'selectionStale') {
        agentSelection.acceptSnapshot(outcome.snapshot);
        return;
      }
      if (outcome.warning === 'writeFailed') {
        toast.warning(t('addSkill.agents.historySaveWarning'));
      }
      agentSelection.confirmCurrentSelection();
      goNext();
    } catch (error) {
      console.error('Failed to confirm Agent selection:', error);
      toast.error(t('addSkill.agents.historySaveError'));
    } finally {
      setConfirmingAgentSelection(false);
    }
  }, [agentSelection, goNext, state.agentSelectionIntent, state.context, t]);

  const goBack = useCallback(() => {
    if (currentStepIndex > 0) {
      goToStep(steps[currentStepIndex - 1]);
    }
  }, [currentStepIndex, steps, goToStep]);

  const closeWizard = useCallback(
    () => requestAction('closeCurrentWindow'),
    [requestAction],
  );

  // 执行失败后回到确认步骤，重新捕获当前 payload 和 runtime preview。
  const handleRetryInstall = useCallback(() => {
    updateState({
      installResults: null,
      installError: undefined,
      preparation: { status: 'idle' },
      step: 'confirm',
    });
    setInstallKey((k) => k + 1);
  }, [updateState]);

  const notifyMainWindow = useCallback((results: InstallResults | null): Promise<boolean> => {
    if (!results || notifiedInstallResultsRef.current === results) {
      return Promise.resolve(true);
    }
    if (notificationInFlightRef.current?.results === results) {
      return notificationInFlightRef.current.promise;
    }
    const mutatedSkillNames = Array.from(new Set(
      results.units
        .filter((unit) => unit.status === 'succeeded')
        .map((unit) => unit.skillName),
    ));
    if (mutatedSkillNames.length === 0) {
      notifiedInstallResultsRef.current = results;
      return Promise.resolve(true);
    }

    const promise = emit('wizard-result', {
      action: 'refresh',
      context: state.context,
      mutatedSkillNames,
    }).then(() => {
      notifiedInstallResultsRef.current = results;
      return true;
    }).catch((error) => {
      console.error('Failed to notify the main window about installed skills:', error);
      return false;
    }).finally(() => {
      if (notificationInFlightRef.current?.results === results) {
        notificationInFlightRef.current = null;
      }
    });
    notificationInFlightRef.current = { results, promise };
    return promise;
  }, [state.context]);

  useEffect(() => {
    void notifyMainWindow(state.installResults);
  }, [notifyMainWindow, state.installResults]);

  // 等待即时通知；若首次发送失败，关闭前再重试一次。
  const handleDone = useCallback(async () => {
    const notified = await notifyMainWindow(state.installResults);
    if (!notified) await notifyMainWindow(state.installResults);
    await closeWizard();
  }, [closeWizard, notifyMainWindow, state.installResults]);

  // 验证是否可以进入下一步
  const canProceed = useMemo(
    () => canProceedForStep(state)
      && (state.step !== 'options' || (
        agentSelection.status === 'ready'
        && !agentSelection.requiresReconfirmation
      )),
    [agentSelection, state],
  );

  // 是否为结果态
  const isResultState = state.step === 'installing' || state.step === 'complete' || state.step === 'error';
  const showActionBar = state.step !== 'installing';
  const hasRetryableResult = state.installResults?.units.some(isRetryableMutationUnit) ?? false;
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
        return (
          <OptionsStep
            agentSelection={agentSelection}
            disabled={confirmingAgentSelection}
          />
        );
      case 'confirm':
        if (agentSelection.status !== 'ready') return null;
        return (
          <ConfirmStep
            state={state}
            agentSelection={agentSelection}
            updateState={updateState}
            scope={state.scope}
            projectPath={state.projectPath}
          />
        );
      case 'installing':
        if (state.preparation.status !== 'ready') return null;
        return (
          <InstallingStep
            key={installKey}
            state={state}
            prepared={state.preparation.prepared}
            updateState={updateState}
            scope={state.scope}
            projectPath={state.projectPath}
          />
        );
      case 'complete':
        return <CompleteStep state={state} />;
      case 'error':
        if (state.installError) {
          return <ErrorStep error={state.installError} />;
        }
        return <CompleteStep state={state} />;
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
              onStepClick={confirmingAgentSelection ? undefined : handleStepClick}
            />
          )}
        </div>

        {/* 底部 Scope 徽章区域 */}
        <div className="px-4 h-[72px] mt-auto border-t bg-muted/5 flex items-center justify-center">
          {showScopeBadge && (
              <ScopeBadge
                scope={state.scope}
                projectPath={state.projectPath}
                environment={state.context.environment}
                environmentName={state.environmentName}
                onClick={confirmingAgentSelection ? undefined : handleScopeBadgeClick}
              />
          )}
        </div>
      </div>

      {/* 右侧主内容区 (Main Content) */}
      <div className="flex-1 flex flex-col min-w-0 bg-background relative">
        {/* 内容滚动区 */}
        <div
          className={cn(
            'flex-1 min-h-0',
            state.step === 'options' ? 'overflow-hidden p-0' : 'overflow-y-auto p-8',
          )}
        >
          <div key={state.step} className="h-full animate-in fade-in slide-in-from-bottom-2 duration-500">
            {renderContent()}
          </div>
        </div>

        {/* 底部操作栏 */}
        {showActionBar && (
          <div className="flex-shrink-0 h-[72px] border-t bg-background/80 backdrop-blur-sm px-8 flex items-center justify-end gap-3 z-10 shadow-[0_-4px_12px_rgba(0,0,0,0.02)]">
            {state.step === 'error' && state.installError ? (
              <>
                <Button variant="outline" onClick={closeWizard}>
                  {t('addSkill.error.actions.close')}
                </Button>
                <Button variant="outline" onClick={() => goToStep(steps[0])}>
                  {t('addSkill.error.actions.backToSource')}
                </Button>
                <Button onClick={handleRetryInstall}>
                  <RotateCw className="h-4 w-4 mr-1.5" />
                  {t('addSkill.error.actions.retry')}
                </Button>
              </>
            ) : isResultState ? (
              <>
                {hasRetryableResult ? (
                  <Button variant="outline" onClick={handleRetryInstall}>
                    {t('addSkill.actions.retry')}
                  </Button>
                ) : null}
                <Button onClick={handleDone}>{t('addSkill.actions.done')}</Button>
              </>
            ) : (
              <>
                <Button
                  variant="outline"
                  onClick={closeWizard}
                  disabled={confirmingAgentSelection}
                >
                  {t('addSkill.actions.cancel')}
                </Button>
                {currentStepIndex > 0 && (
                  <Button
                    variant="outline"
                    onClick={goBack}
                    disabled={confirmingAgentSelection}
                  >
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
                  <Button
                    onClick={state.step === 'options' ? handleOptionsNext : goNext}
                    disabled={!canProceed || confirmingAgentSelection}
                    className="min-w-[100px]"
                  >
                    {t('addSkill.actions.next')}
                  </Button>
                )}
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
