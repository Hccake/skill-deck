// src/components/skills/add-skill/CompleteStep.tsx
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2, XCircle, AlertTriangle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { AddSkillState } from './types';

interface CompleteStepProps {
  state: AddSkillState;
  onDone: () => void;
  onRetry: () => void;
}

export function CompleteStep({ state, onDone, onRetry }: CompleteStepProps) {
  const { t } = useTranslation();
  const results = state.installResults;

  if (!results) {
    return null;
  }

  const failedCount = results.failed.length;
  const hasFailures = failedCount > 0;
  const hasSymlinkFallback = results.symlinkFallbackAgents.length > 0;

  // 合并两个 useMemo 为单次遍历，符合 js-combine-iterations 规则
  const { successfulBySkill, failedBySkill } = useMemo(() => {
    const successMap = new Map<string, typeof results.successful>();
    const failedMap = new Map<string, typeof results.failed>();

    for (const r of results.successful) {
      const existing = successMap.get(r.skillName) ?? [];
      existing.push(r);
      successMap.set(r.skillName, existing);
    }

    for (const r of results.failed) {
      const existing = failedMap.get(r.skillName) ?? [];
      existing.push(r);
      failedMap.set(r.skillName, existing);
    }

    return { successfulBySkill: successMap, failedBySkill: failedMap };
  }, [results.successful, results.failed]);

  return (
    <div className="space-y-4 py-4">
      {/* Header */}
      <div className="flex items-center gap-3">
        {hasFailures ? (
          <XCircle className="h-6 w-6 text-destructive" />
        ) : (
          <CheckCircle2 className="h-6 w-6 text-green-600" />
        )}
        <h3 className="text-lg font-medium">
          {hasFailures
            ? t('addSkill.complete.partial')
            : t('addSkill.complete.success', { count: successfulBySkill.size })}
        </h3>
      </div>

      {/* Counts */}
      {hasFailures && (
        <div className="flex gap-4 text-sm">
          <span className="text-green-600">
            {t('addSkill.complete.successCount', { count: successfulBySkill.size })}
          </span>
          <span className="text-destructive">
            {t('addSkill.complete.failedCount', { count: failedBySkill.size })}
          </span>
        </div>
      )}

      {/* Results list */}
      <div className="border rounded-md p-3 space-y-2">
          {/* Successful */}
          {Array.from(successfulBySkill.entries()).map(([skillName, items]) => {
            const first = items[0];
            const symlinkFailed = items.some((i) => i.symlinkFailed);

            return (
              <div
                key={skillName}
                className="flex items-start gap-2 p-2 bg-muted/30 rounded-md"
              >
                <CheckCircle2 className="h-4 w-4 text-green-600 mt-0.5" />
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium">{first?.canonicalPath ?? skillName}</div>
                  {symlinkFailed && (
                    <div className="text-xs text-amber-600">
                      {t('addSkill.complete.copied')}
                    </div>
                  )}
                </div>
              </div>
            );
          })}

          {/* Failed */}
          {Array.from(failedBySkill.entries()).map(([skillName, items]) => {
            const first = items[0];

            return (
              <div
                key={skillName}
                className="flex items-start gap-2 p-2 bg-destructive/10 rounded-md"
              >
                <XCircle className="h-4 w-4 text-destructive mt-0.5" />
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium">{skillName}</div>
                  <div className="text-xs text-destructive">
                    {first?.error ?? 'Unknown error'}
                  </div>
                </div>
              </div>
            );
          })}
      </div>

      {/* Symlink fallback warning */}
      {hasSymlinkFallback && (
        <div className="flex items-start gap-2 p-3 bg-amber-500/10 text-amber-700 dark:text-amber-400 text-sm rounded-md">
          <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5" />
          <div>
            <div>
              {t('addSkill.complete.symlinkFailed', {
                agents: results.symlinkFallbackAgents.join(', '),
              })}
            </div>
            <div className="text-xs opacity-80">
              {t('addSkill.complete.symlinkFailedHint')}
            </div>
          </div>
        </div>
      )}

      {/* Actions */}
      <div className="flex justify-end gap-2 pt-2">
        {hasFailures && (
          <Button variant="outline" onClick={onRetry}>
            {t('addSkill.actions.retry')}
          </Button>
        )}
        <Button onClick={onDone}>{t('addSkill.actions.done')}</Button>
      </div>
    </div>
  );
}
