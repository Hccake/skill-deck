// src/components/skills/AddSkillDialog.tsx
import { useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Globe, Folder } from 'lucide-react';
import { SourceStep } from './add-skill/SourceStep';
import { SkillsStep } from './add-skill/SkillsStep';
import { OptionsStep } from './add-skill/OptionsStep';
import { ConfirmStep } from './add-skill/ConfirmStep';
import { InstallingStep } from './add-skill/InstallingStep';
import { CompleteStep } from './add-skill/CompleteStep';
import type { AddSkillState, WizardStep } from './add-skill/types';

interface AddSkillDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 安装范围，由触发按钮所在区块决定，不可更改 */
  scope: 'global' | 'project';
  projectPath?: string;
  onInstallComplete?: () => void;
}

/** 从完整路径中提取项目名称 */
function getProjectName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] || path;
}

const STEP_LABELS = ['source', 'skills', 'options', 'confirm'] as const;

// 初始状态工厂函数 - 符合 rerender-lazy-state-init 规则
function createInitialState(): AddSkillState {
  return {
    step: 1,
    source: '',
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

export function AddSkillDialog({
  open,
  onOpenChange,
  scope,
  projectPath,
  onInstallComplete,
}: AddSkillDialogProps) {
  const { t } = useTranslation();

  // 使用函数初始化避免每次渲染重新创建对象
  const [state, setState] = useState<AddSkillState>(() =>
    createInitialState()
  );

  // 重置状态
  const resetState = useCallback(() => {
    setState(createInitialState());
  }, []);

  // 关闭弹窗时重置
  const handleOpenChange = useCallback((open: boolean) => {
    if (!open) {
      resetState();
    }
    onOpenChange(open);
  }, [onOpenChange, resetState]);

  // 更新状态的 helper - 支持函数式更新，符合 rerender-functional-setstate 规则
  const updateState = useCallback(
    (updates: Partial<AddSkillState> | ((prev: AddSkillState) => Partial<AddSkillState>)) => {
      setState(prev => ({
        ...prev,
        ...(typeof updates === 'function' ? updates(prev) : updates),
      }));
    },
    []
  );

  // 导航
  const goToStep = useCallback((step: WizardStep) => {
    updateState({ step });
  }, [updateState]);

  const goBack = useCallback(() => {
    const currentStep = state.step;
    if (typeof currentStep === 'number' && currentStep > 1) {
      goToStep((currentStep - 1) as WizardStep);
    }
  }, [state.step, goToStep]);

  const goNext = useCallback(() => {
    const currentStep = state.step;
    if (typeof currentStep === 'number' && currentStep < 4) {
      goToStep((currentStep + 1) as WizardStep);
    }
  }, [state.step, goToStep]);

  // 渲染步骤指示器
  const renderStepper = () => {
    const currentStep = typeof state.step === 'number' ? state.step : 0;

    return (
      <div className="flex items-center justify-center gap-2 pt-2">
        {STEP_LABELS.map((label, index) => {
          const stepNum = index + 1;
          const isActive = stepNum === currentStep;
          const isCompleted = stepNum < currentStep;

          return (
            <div key={label} className="flex items-center gap-2">
              {index > 0 && (
                <div className={`h-px w-8 ${isCompleted ? 'bg-primary' : 'bg-muted'}`} />
              )}
              <div className="flex flex-col items-center gap-1">
                <div
                  className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-medium ${
                    isActive
                      ? 'bg-primary text-primary-foreground'
                      : isCompleted
                      ? 'bg-primary/20 text-primary'
                      : 'bg-muted text-muted-foreground'
                  }`}
                >
                  {stepNum}
                </div>
                <span className={`text-xs ${isActive ? 'text-foreground' : 'text-muted-foreground'}`}>
                  {t(`addSkill.steps.${label}`)}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    );
  };

  // 渲染当前步骤内容
  const renderStepContent = () => {
    switch (state.step) {
      case 1:
        return (
          <SourceStep
            state={state}
            updateState={updateState}
            onNext={goNext}
          />
        );
      case 2:
        return (
          <SkillsStep
            state={state}
            updateState={updateState}
          />
        );
      case 3:
        return (
          <OptionsStep
            state={state}
            updateState={updateState}
          />
        );
      case 4:
        return (
          <ConfirmStep
            state={state}
            updateState={updateState}
            scope={scope}
            projectPath={projectPath}
          />
        );
      case 'installing':
        return (
          <InstallingStep
            state={state}
            updateState={updateState}
            scope={scope}
            projectPath={projectPath}
          />
        );
      case 'complete':
      case 'error':
        return (
          <CompleteStep
            state={state}
            onDone={() => {
              handleOpenChange(false);
              onInstallComplete?.();
            }}
            onRetry={() => goToStep(4)}
          />
        );
      default:
        return null;
    }
  };

  // 渲染底部按钮
  const renderFooter = () => {
    const { step } = state;

    // 安装中和完成状态不显示底部按钮
    if (step === 'installing' || step === 'complete' || step === 'error') {
      return null;
    }

    const canGoBack = typeof step === 'number' && step > 1;
    const canGoNext = typeof step === 'number' && step < 4;
    const isConfirmStep = step === 4;

    // 验证是否可以进入下一步
    const canProceed = (() => {
      switch (step) {
        case 1:
          return state.fetchStatus === 'success' && state.availableSkills.length > 0;
        case 2:
          return state.selectedSkills.length > 0;
        case 3:
          return true; // Options 总是可以进入确认
        default:
          return true;
      }
    })();

    return (
      <div className="flex justify-end gap-2 px-6 py-3 border-t">
          <Button variant="outline" onClick={() => handleOpenChange(false)}>
            {t('addSkill.actions.cancel')}
          </Button>
          {canGoBack && (
            <Button variant="outline" onClick={goBack}>
              {t('addSkill.actions.back')}
            </Button>
          )}
          {canGoNext && (
            <Button onClick={goNext} disabled={!canProceed}>
              {t('addSkill.actions.next')}
            </Button>
          )}
          {isConfirmStep && (
            <Button onClick={() => goToStep('installing')} disabled={!canProceed}>
              {t('addSkill.actions.install')}
            </Button>
          )}
        </div>
    );
  };

  // 仅在 stepper 步骤显示步骤指示器
  const showStepper = typeof state.step === 'number';

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="w-[90vw] max-w-3xl max-h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {t('addSkill.title')}
            <Badge variant="outline" className="font-normal text-xs">
              {scope === 'global' ? (
                <><Globe className="w-3 h-3 mr-1" />{t('addSkill.scope.global')}</>
              ) : (
                <><Folder className="w-3 h-3 mr-1" />{getProjectName(projectPath ?? '')}</>
              )}
            </Badge>
          </DialogTitle>
        </DialogHeader>

        {showStepper && renderStepper()}

        <div className="flex-1 overflow-auto min-h-0 px-1">
          {renderStepContent()}
        </div>

        {renderFooter()}
      </DialogContent>
    </Dialog>
  );
}
