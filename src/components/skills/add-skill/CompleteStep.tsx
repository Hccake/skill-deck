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

function resultCategory(result: InstallResult) {
  if (result.category) return result.category;
  if (result.skipped) return 'skipped';
  if (!result.success) return 'failed';
  return 'private-adapted';
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
      const targetMap = resultCategory(r) === 'skipped' ? skippedMap : successMap;
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
  const defaultAvailableAgents = results.defaultAvailableAgents;
  const privateCopyAgentIds = new Set(results.privateCopyAgents ?? []);
  const defaultAvailableAgentCount = defaultAvailableAgents
    ? defaultAvailableAgents.filter((agent) => !privateCopyAgentIds.has(agent)).length
    : null;

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
          const hasSkillFailures = group.failed.length > 0;
          const expanded = expandedSkills[group.skillName] === true;
          const defaultAvailable = group.successful.filter((r) => resultCategory(r) === 'default-available');
          const privateAdapted = group.successful.filter((r) => resultCategory(r) === 'private-adapted');
          const privateCopies = group.successful.filter((r) => resultCategory(r) === 'private-copy');
          const defaultAvailableCount = defaultAvailable.length > 0
            ? defaultAvailableAgentCount ?? defaultAvailable.length
            : 0;
          const successCount = defaultAvailableCount + privateAdapted.length + privateCopies.length;
          const totalCount = successCount + group.skipped.length + group.failed.length;

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
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {defaultAvailableCount > 0 && (
                      <ResultCategoryBadge
                        label={t('addSkill.complete.defaultAvailable')}
                        count={defaultAvailableCount}
                      />
                    )}
                    {privateAdapted.length > 0 && (
                      <ResultCategoryBadge
                        label={t('addSkill.complete.privateAdapted')}
                        count={privateAdapted.length}
                      />
                    )}
                    {privateCopies.length > 0 && (
                      <ResultCategoryBadge
                        label={t('addSkill.complete.privateCopies')}
                        count={privateCopies.length}
                      />
                    )}
                    {group.skipped.length > 0 && (
                      <ResultCategoryBadge
                        label={t('addSkill.complete.skippedCategory')}
                        count={group.skipped.length}
                      />
                    )}
                    {group.failed.length > 0 && (
                      <ResultCategoryBadge
                        label={t('addSkill.complete.failedCategory')}
                        count={group.failed.length}
                      />
                    )}
                  </div>
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

function ResultCategoryBadge({ label, count }: { label: string; count: number }) {
  return (
    <span className="inline-flex items-center rounded border border-border/60 bg-background/70 px-1.5 py-0.5 text-[11px] leading-none text-muted-foreground">
      {label} · {count}
    </span>
  );
}
