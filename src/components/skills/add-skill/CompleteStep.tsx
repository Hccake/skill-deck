// src/components/skills/add-skill/CompleteStep.tsx
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2, XCircle, AlertTriangle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { InstallResult } from '@/bindings';
import type { WizardState } from './types';

interface CompleteStepProps {
  state: WizardState;
  onDone: () => void;
  onRetry?: () => void;
  onRetrySkill?: (skillName: string, failedAgents: string[]) => void;
}

interface SkillGroup {
  skillName: string;
  successful: InstallResult[];
  skipped: InstallResult[];
  failed: InstallResult[];
}

export function CompleteStep({ state, onDone, onRetry, onRetrySkill }: CompleteStepProps) {
  const { t } = useTranslation();
  const results = state.installResults;
  const [expandedSkills, setExpandedSkills] = useState<Record<string, boolean>>({});

  // useMemo 必须在 early return 之前调用（rules-of-hooks）
  const { groups, successfulSkillCount, failedSkillCount } = useMemo(() => {
    if (!results) {
      return {
        groups: [] as SkillGroup[],
        successfulSkillCount: 0,
        failedSkillCount: 0,
      };
    }

    const successMap = new Map<string, InstallResult[]>();
    const skippedMap = new Map<string, InstallResult[]>();
    const failedMap = new Map<string, InstallResult[]>();

    for (const r of results.successful) {
      const targetMap = r.skipped ? skippedMap : successMap;
      const existing = targetMap.get(r.skillName) ?? [];
      existing.push(r);
      targetMap.set(r.skillName, existing);
    }

    for (const r of results.failed) {
      const existing = failedMap.get(r.skillName) ?? [];
      existing.push(r);
      failedMap.set(r.skillName, existing);
    }

    const allSkillNames = Array.from(
      new Set([...successMap.keys(), ...skippedMap.keys(), ...failedMap.keys()])
    ).sort((a, b) => a.localeCompare(b));
    const grouped = allSkillNames.map((skillName) => ({
      skillName,
      successful: successMap.get(skillName) ?? [],
      skipped: skippedMap.get(skillName) ?? [],
      failed: failedMap.get(skillName) ?? [],
    }));

    return {
      groups: grouped,
      successfulSkillCount: grouped.filter((g) => g.failed.length === 0).length,
      failedSkillCount: grouped.filter((g) => g.failed.length > 0).length,
    };
  }, [results]);

  if (!results) {
    return null;
  }

  const hasFailures = failedSkillCount > 0;
  const hasSymlinkFallback = results.symlinkFallbackAgents.length > 0;

  const toggleSkill = (skillName: string) => {
    setExpandedSkills((prev) => ({
      ...prev,
      [skillName]: !prev[skillName],
    }));
  };

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center gap-3">
        {hasFailures ? (
          <XCircle className="h-6 w-6 text-destructive" />
        ) : (
          <CheckCircle2 className="h-6 w-6 text-green-600" />
        )}
        <h3 className="text-lg font-heading font-bold">
          {hasFailures
            ? t('addSkill.complete.partial')
            : t('addSkill.complete.success', { count: successfulSkillCount })}
        </h3>
      </div>

      {/* Counts */}
      {hasFailures && (
        <div className="flex gap-4 text-sm">
          <span className="text-green-600">
            {t('addSkill.complete.successCount', { count: successfulSkillCount })}
          </span>
          <span className="text-destructive">
            {t('addSkill.complete.failedCount', { count: failedSkillCount })}
          </span>
        </div>
      )}

      {/* Results list */}
      <div className="border rounded-md p-3 space-y-2">
        {groups.map((group) => {
          const successCount = group.successful.length;
          const totalCount = group.successful.length + group.skipped.length + group.failed.length;
          const hasSkillFailures = group.failed.length > 0;
          const expanded = expandedSkills[group.skillName] === true;

          return (
            <div
              key={group.skillName}
              className={`rounded-md border p-2 ${
                hasSkillFailures ? 'border-destructive/30 bg-destructive/5' : 'border-border bg-muted/30'
              }`}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    {hasSkillFailures ? (
                      <XCircle className="h-4 w-4 text-destructive shrink-0" />
                    ) : (
                      <CheckCircle2 className="h-4 w-4 text-green-600 shrink-0" />
                    )}
                    <span className="text-sm font-medium break-all">{group.skillName}</span>
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {t('addSkill.complete.agentCoverage', {
                      success: successCount,
                      total: totalCount,
                    })}
                  </div>
                  {group.skipped.length > 0 && (
                    <div className="mt-1 text-xs text-muted-foreground">
                      {t('addSkill.complete.skipped', {
                        agents: group.skipped.map((item) => item.agent).join(', '),
                      })}
                    </div>
                  )}
                </div>

                {hasSkillFailures && (
                  <div className="flex items-center gap-1.5">
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-7"
                      onClick={() => toggleSkill(group.skillName)}
                    >
                      {expanded
                        ? t('addSkill.complete.hideFailures')
                        : t('addSkill.complete.showFailures', { count: group.failed.length })}
                    </Button>
                    {onRetrySkill && (
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7"
                        onClick={() =>
                          onRetrySkill(
                            group.skillName,
                            group.failed.map((f) => f.agent)
                          )
                        }
                      >
                        {t('addSkill.actions.retrySkill')}
                      </Button>
                    )}
                  </div>
                )}
              </div>

              {hasSkillFailures && expanded && (
                <div className="mt-2 space-y-1 rounded-md bg-background/70 p-2">
                  {group.failed.map((item) => (
                    <div key={`${group.skillName}-${item.agent}`} className="text-xs">
                      <div className="font-medium text-destructive">{item.agent}</div>
                      <div className="text-destructive/90 break-words">
                        {item.error ?? t('addSkill.error.unknown')}
                      </div>
                    </div>
                  ))}
                </div>
              )}
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
        {hasFailures && onRetry && (
          <Button variant="outline" onClick={onRetry}>
            {t('addSkill.actions.retry')}
          </Button>
        )}
        <Button onClick={onDone}>{t('addSkill.actions.done')}</Button>
      </div>
    </div>
  );
}
