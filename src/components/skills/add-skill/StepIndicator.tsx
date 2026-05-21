// src/components/skills/add-skill/StepIndicator.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Check } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { EntryPoint, CoreStep, WizardStep } from './types';
import { getStepFlow } from './types';

interface StepIndicatorProps {
  entryPoint: EntryPoint;
  currentStep: WizardStep;
  onStepClick?: (step: CoreStep) => void;
  orientation?: 'horizontal' | 'vertical';
}

export const StepIndicator = memo(function StepIndicator({
  entryPoint,
  currentStep,
  onStepClick,
  orientation = 'horizontal',
}: StepIndicatorProps) {
  const { t } = useTranslation();
  const steps = getStepFlow(entryPoint);

  const currentIndex = steps.indexOf(currentStep as CoreStep);
  // 结果态时所有步骤都显示为已完成
  const isResultState = currentIndex === -1;
  const isVertical = orientation === 'vertical';

  return (
    <div
      className={cn(
        'flex',
        isVertical ? 'flex-col items-start gap-0 py-6 px-4' : 'items-center justify-center gap-2 py-3 px-4'
      )}
    >
      {steps.map((step, index) => {
        const isCompleted = isResultState || index < currentIndex;
        const isActive = index === currentIndex;
        const canClick = isCompleted && onStepClick && !isResultState;

        return (
          <div
            key={step}
            className={cn(
              'flex',
              isVertical ? 'flex-col items-start' : 'items-center gap-2'
            )}
          >
            {index > 0 && (
              <div
                className={cn(
                  isVertical ? 'w-px h-6 ml-3 my-1' : 'h-px w-8',
                  isCompleted || isActive ? 'bg-primary' : 'bg-muted'
                )}
              />
            )}
            <button
              type="button"
              disabled={!canClick}
              onClick={() => canClick && onStepClick?.(step)}
              className={cn(
                'flex items-center',
                isVertical ? 'flex-row gap-3' : 'flex-col gap-1',
                canClick ? 'cursor-pointer' : 'cursor-default'
              )}
            >
              <div
                className={cn(
                  'w-6 h-6 rounded-full flex items-center justify-center text-xs font-medium transition-colors shrink-0',
                  isActive && 'bg-primary text-primary-foreground shadow-sm',
                  isCompleted && 'bg-primary/20 text-primary',
                  !isActive && !isCompleted && 'bg-muted text-muted-foreground'
                )}
              >
                {isCompleted ? <Check className="w-3.5 h-3.5" /> : index + 1}
              </div>
              <span
                className={cn(
                  'text-sm transition-colors',
                  isActive ? 'text-foreground font-medium' : 'text-muted-foreground',
                  !isVertical && 'text-xs'
                )}
              >
                {t(`addSkill.steps.${step}`)}
              </span>
            </button>
          </div>
        );
      })}
    </div>
  );
});
