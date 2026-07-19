import { useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { useMutationStore } from '@/stores/mutation';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';
import { presentMutationUnit } from '@/workflows/mutation-presentation';
import {
  formatFallbackReason,
  formatMutationError,
  formatMutationWarning,
} from '@/lib/mutation-results';
import type { AgentId, ContextRef, ErrorReport, UpdateSkillResult } from '@/bindings';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { contextKey } from '@/lib/context';
import { formatAppError } from '@/utils/format-app-error';

const EMPTY_RESULTS: UpdateSkillResult[] = [];

interface UpdatePlanDialogProps {
  open: boolean;
  context: ContextRef | null;
  skillNames: string[];
  agentDisplayNames?: Map<AgentId, string>;
  onOpenChange: (open: boolean) => void;
}

function AgentList({
  agents,
  agentDisplayNames,
}: {
  agents: AgentId[];
  agentDisplayNames?: Map<AgentId, string>;
}) {
  const visibleAgents = agents.slice(0, 3);
  const hiddenCount = agents.length - visibleAgents.length;

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-1 text-xs text-muted-foreground sm:justify-end">
      {visibleAgents.map((agent, index) => (
        <span key={agent} className="inline-flex items-center gap-x-1.5">
          <span className="truncate">{agentDisplayNames?.get(agent) ?? agent}</span>
          {index < visibleAgents.length - 1 ? <span className="text-muted-foreground/40">·</span> : null}
        </span>
      ))}
      {hiddenCount > 0 ? (
        <span className="rounded-full bg-muted px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground">
          +{hiddenCount}
        </span>
      ) : null}
    </div>
  );
}

function isCancelled(report: ErrorReport | null | undefined): boolean {
  return report?.code === 'mutationCancelled';
}

function updateSkillStatus(item: UpdateSkillResult, sourceError?: ErrorReport | null): string {
  if (item.mutation) return item.mutation.status;
  if (
    (item.coverage.kind === 'notUpdated' && isCancelled(item.coverage.error))
    || isCancelled(sourceError)
  ) {
    return 'cancelled';
  }
  return 'failed';
}

export function UpdatePlanDialog({
  open,
  context,
  skillNames,
  agentDisplayNames,
  onOpenChange,
}: UpdatePlanDialogProps) {
  const { t } = useTranslation();
  const phase = useSkillUpdateWorkflow((s) => s.phase);
  const preview = useSkillUpdateWorkflow((s) => s.preview);
  const result = useSkillUpdateWorkflow((s) => s.result);
  const executionError = useSkillUpdateWorkflow((s) => s.executionError);
  const confirming = useSkillUpdateWorkflow((s) => s.confirming);
  const decisions = useSkillUpdateWorkflow((s) => s.conflictDecisions);
  const setConflictDecision = useSkillUpdateWorkflow((s) => s.setConflictDecision);
  const confirmWorkflow = useSkillUpdateWorkflow((s) => s.confirm);
  const retryWorkflow = useSkillUpdateWorkflow((s) => s.retryFailed);
  const retryPreview = useSkillUpdateWorkflow((s) => s.open);
  const acceptMutation = useSkillUpdateWorkflow((s) => s.acceptMutation);
  const displayResponse = result;
  const displayResults = result?.skills ?? EMPTY_RESULTS;
  const displayPreview = preview;
  const activeMutation = useMutationStore((state) => state.activeMutation);
  const cancelActiveMutation = useMutationStore((state) => state.cancelActiveMutation);
  const matchingUpdateMutation = context !== null && activeMutation?.kind === 'update'
    && contextKey(activeMutation.context) === contextKey(context);
  const irreversibleUpdate = matchingUpdateMutation && !activeMutation.cancelable;
  const closeBlocked = irreversibleUpdate || (confirming && !matchingUpdateMutation);
  const writeBlocked = activeMutation !== null;
  const environments = useEnvironmentStore((state) => state.environments);
  const projectsByEnvironment = useProjectStore((state) => state.projectsByEnvironment);

  useEffect(() => {
    acceptMutation(activeMutation);
  }, [acceptMutation, activeMutation]);

  const resultCounts = useMemo(() => {
    const results = displayResults;
    const sourceErrors = new Map(
      displayResponse?.sources.map((source) => [source.id, source.error]) ?? [],
    );
    return {
      success: results.filter((item) => item.mutation?.status === 'succeeded').length,
      partial: results.filter((item) => (
        item.coverage.kind === 'preservedConflicts'
        || item.mutation?.status === 'notRun'
        || item.mutation?.status === 'skipped'
      )).length,
      failed: results.filter((item) => (
        updateSkillStatus(item, sourceErrors.get(item.sourceResultId)) === 'failed'
      )).length,
      skipped: results.filter((item) => (
        updateSkillStatus(item, sourceErrors.get(item.sourceResultId)) === 'cancelled'
      )).length,
    };
  }, [displayResponse, displayResults]);
  const retryableResults = useMemo(
    () => displayResults.filter((item) => item.retryable),
    [displayResults],
  );
  const resultPresentations = useMemo(
    () => new Map(displayResults.flatMap((item) => item.mutation ? [[
      item.skillIdentity.skillName,
      presentMutationUnit(item.mutation, t, { environments, projectsByEnvironment }),
    ] as const] : [])),
    [environments, displayResults, projectsByEnvironment, t],
  );
  const privateEntries = useMemo(
    () => displayPreview?.skills.flatMap((skill) => skill.overwritePrivateEntries) ?? [],
    [displayPreview],
  );
  const ownerDisplayNameCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const entry of privateEntries) {
      for (const owner of entry.owners) {
        counts.set(owner.displayName, (counts.get(owner.displayName) ?? 0) + 1);
      }
    }
    return counts;
  }, [privateEntries]);
  const cleanCopyCounts = useMemo(
    () => displayPreview?.skills.filter((skill) => skill.cleanCopyCount > 0) ?? [],
    [displayPreview],
  );
  const previewGroups = useMemo(() => {
    const groups = new Map<string, {
      sourceDisplay: string;
      refDisplay: string;
      skills: NonNullable<typeof displayPreview>['skills'];
    }>();
    for (const skill of displayPreview?.skills ?? []) {
      const key = `${skill.sourceDisplay}\u0000${skill.refDisplay}`;
      const group = groups.get(key) ?? {
        sourceDisplay: skill.sourceDisplay,
        refDisplay: skill.refDisplay,
        skills: [],
      };
      group.skills.push(skill);
      groups.set(key, group);
    }
    return [...groups.values()];
  }, [displayPreview]);

  if (!context) return null;
  const canConfirm = preview?.skills.some((skill) => (
    skill.capability.canRunUpdate && skill.blockingReasons.length === 0
  )) ?? false;
  const readyTitleCount = preview
    ? preview.skills.filter((skill) => (
      skill.capability.canRunUpdate && skill.blockingReasons.length === 0
    )).length
    : skillNames.length;
  const phaseStatus = phase === 'acquiring' || phase === 'validating' || phase === 'updating'
    ? phase
    : null;
  const progress = matchingUpdateMutation ? activeMutation.progress : null;

  const handleConfirm = async () => {
    await confirmWorkflow();
  };

  const handleRetryFailed = async () => {
    await retryWorkflow();
  };

  const handlePreviewRetry = async () => {
    await retryPreview(context, skillNames);
  };

  const handleOpenChange = (nextOpen: boolean) => {
    if (nextOpen) return onOpenChange(true);
    if (closeBlocked) return;
    if (matchingUpdateMutation) {
      void cancelActiveMutation();
      return;
    }
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        className="sm:max-w-2xl gap-0"
        showCloseButton={!closeBlocked}
        aria-busy={phase === 'loadingPreview' || phase === 'acquiring' || phase === 'validating' || phase === 'updating'}
      >
        <DialogHeader className="pb-4 border-b border-border">
          <DialogTitle>{t('skills.updatePlan.readyTitle', { count: readyTitleCount })}</DialogTitle>
          <DialogDescription>{t('skills.updatePlan.readyDescription')}</DialogDescription>
        </DialogHeader>

        <div className="py-4 space-y-4 max-h-[60vh] overflow-y-auto">
          {phase === 'loadingPreview' ? (
            <div className="py-8 text-sm text-muted-foreground" role="status">
              {t('skills.updatePlan.loadingPreview')}
            </div>
          ) : phase === 'previewError' ? (
            <div className="py-8 text-sm text-destructive" role="alert">
              {t('skills.updatePlan.previewError')}
            </div>
          ) : phase !== 'result' ? (
            <>
              {phaseStatus ? (
                <div className="rounded-md border border-primary/30 bg-primary/5 px-3 py-2 text-sm" role="status" aria-live="polite">
                  <p className="font-medium">{t(`mutation.phase.${phaseStatus}`)}</p>
                  {progress?.current != null && progress.total != null ? (
                    <p className="mt-1 text-xs text-muted-foreground">
                      {t('skills.updatePlan.progress', { current: progress.current, total: progress.total })}
                    </p>
                  ) : null}
                </div>
              ) : null}
              {cleanCopyCounts.length > 0 ? (
                <section className="space-y-1" aria-label={t('skills.updatePlan.cleanCopyCount')}>
                  <p className="text-sm text-muted-foreground">
                    {t('skills.updatePlan.cleanCopyCount', {
                      count: cleanCopyCounts.reduce((total, skill) => total + skill.cleanCopyCount, 0),
                    })}
                  </p>
                  {cleanCopyCounts.map((skill) => (
                    <p key={skill.skillName} className="text-xs text-muted-foreground">
                      {t('skills.updatePlan.cleanCopyCountForSkill', {
                        skillName: skill.skillName,
                        count: skill.cleanCopyCount,
                      })}
                    </p>
                  ))}
                </section>
              ) : null}
              {privateEntries.length > 0 ? (
                <section className="space-y-2" aria-labelledby="update-private-entries-title">
                  <div>
                    <p id="update-private-entries-title" className="text-sm font-medium">
                      {t('skills.updatePlan.privateCopiesTitle')}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      {t('skills.updatePlan.privateCopiesDescription')}
                    </p>
                  </div>
                  {privateEntries.map((entry, entryIndex) => {
                    const checked = decisions.has(entry.entryId);
                    return (
                      <label
                        key={entry.entryId}
                        className="flex min-w-0 cursor-pointer items-start gap-3 rounded-md border border-border/70 p-3"
                      >
                        <Checkbox
                          checked={checked}
                          aria-label={t('skills.updatePlan.overwritePrivateEntry')}
                          onCheckedChange={(nextChecked) => {
                            setConflictDecision(entry.entryId, nextChecked === true);
                          }}
                        />
                        <span className="flex min-w-0 flex-1 items-start justify-between gap-3 text-xs text-muted-foreground">
                          <span className="flex min-w-0 flex-wrap gap-x-1">
                            {entry.owners.map((owner, index) => (
                              <span key={`${entry.entryId}:${owner.logicalTargetId}`} className="inline-flex flex-wrap gap-x-1">
                                <span>{owner.displayName}</span>
                                {(ownerDisplayNameCounts.get(owner.displayName) ?? 0) > 1 ? (
                                  <span>{owner.agentId} - {owner.logicalTargetId}</span>
                                ) : null}
                                {index < entry.owners.length - 1 ? <span aria-hidden="true">·</span> : null}
                              </span>
                            ))}
                          </span>
                          <span className="shrink-0">#{entryIndex + 1}</span>
                        </span>
                      </label>
                    );
                  })}
                </section>
              ) : null}
              {previewGroups.map((group) => (
                <div
                  key={`${group.sourceDisplay}:${group.refDisplay}`}
                  className="overflow-hidden rounded-md border border-border/70 bg-background/60"
                >
                  <div className="flex items-center justify-between gap-3 border-b border-border/60 px-3 py-2.5">
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium">{group.sourceDisplay}</p>
                      {group.refDisplay ? (
                        <p className="mt-0.5 text-xs text-muted-foreground">
                          {t('skills.refBadge', { ref: group.refDisplay })}
                        </p>
                      ) : null}
                    </div>
                    <span className="shrink-0 rounded-full bg-muted px-2 py-1 text-xs font-medium text-muted-foreground">
                      {t('skills.updatePlan.skillCount', { count: group.skills.length })}
                    </span>
                  </div>
                  <div className="divide-y divide-border/60">
                    {group.skills.map((skill) => (
                      <div
                        key={skill.skillName}
                        className="grid gap-1.5 px-3 py-2.5 sm:grid-cols-[minmax(0,1fr)_minmax(12rem,1.15fr)] sm:items-center"
                      >
                        <span className="min-w-0 truncate text-sm font-medium">{skill.skillName}</span>
                        <AgentList agents={skill.placementAgentIds} agentDisplayNames={agentDisplayNames} />
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </>
          ) : null}

          {phase === 'result' ? (
            <div className="rounded-md border border-border p-3" role="status" aria-live="polite">
              <div className="flex items-center gap-2 text-sm font-medium">
                <CheckCircle2 className="h-4 w-4 text-success" />
                {t('skills.updatePlan.resultTitle')}
                {displayResponse ? (
                  <Badge variant="outline" className="text-xs">
                    {t(`skills.updatePlan.resultOutcome.${displayResponse.outcome}`)}
                  </Badge>
                ) : null}
              </div>
              {executionError ? (
                <p className="mt-2 text-sm text-destructive" role="alert">{formatAppError(executionError, t)}</p>
              ) : (
                <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
                  <span>{t('skills.updatePlan.resultSuccess', { count: resultCounts.success })}</span>
                  <span>{t('skills.updatePlan.resultPartial', { count: resultCounts.partial })}</span>
                  <span>{t('skills.updatePlan.resultFailed', { count: resultCounts.failed })}</span>
                  <span>{t('skills.updatePlan.resultSkipped', { count: resultCounts.skipped })}</span>
                </div>
              )}
              {!executionError && displayResults.length ? (
                <div className="mt-3 space-y-2">
                  {displayResponse?.sources.map((source) => {
                    const status = isCancelled(source.error) ? 'cancelled' : source.status;
                    return (
                      <div key={source.id} className="rounded-md border border-border/70 p-2">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-sm font-medium">{source.source}</span>
                          <Badge variant="outline" className="text-xs">
                            {t(`mutation.result.status.${status}`)}
                          </Badge>
                        </div>
                        {source.error ? (
                          <p className="mt-1 text-xs text-destructive" role="alert">
                            {formatMutationError(source.error, t)}
                          </p>
                        ) : null}
                      </div>
                    );
                  })}
                  {displayResults.map((item) => {
                    const mutation = item.mutation;
                    const presentation = resultPresentations.get(item.skillIdentity.skillName);
                    const source = displayResponse?.sources.find(
                      (candidate) => candidate.id === item.sourceResultId,
                    );
                    const coverageError = item.coverage.kind === 'notUpdated'
                      ? item.coverage.error
                      : null;
                    const error = mutation?.error
                      ?? (source?.error?.code === coverageError?.code ? null : coverageError);
                    const status = updateSkillStatus(item, source?.error);
                    return (
                      <div key={item.skillIdentity.skillName} className="rounded-md border border-border/70 p-2">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="text-sm font-medium">{item.skillIdentity.skillName}</span>
                        <Badge variant="outline" className="text-xs">
                          {t(`mutation.result.status.${status}`)}
                        </Badge>
                        {source ? <span className="text-xs text-muted-foreground">{source.source}</span> : null}
                        {mutation?.fallbackReason ? (
                          <span className="text-xs text-muted-foreground">
                            {formatFallbackReason(mutation.fallbackReason, t)}
                          </span>
                        ) : null}
                      </div>
                      {presentation ? <p className="mt-1 text-xs text-muted-foreground">
                        {t('mutation.result.location', {
                          environment: presentation.environmentLabel,
                          scope: presentation.scopeLabel,
                        })}
                      </p> : null}
                      {error ? (
                        <p className="mt-1 text-xs text-destructive" role="alert">
                          {formatMutationError(error, t)}
                        </p>
                      ) : null}
                      {mutation?.warnings.map((warning, index) => (
                        <p key={`${item.skillIdentity.skillName}:warning:${warning.code}:${index}`} className="mt-1 text-xs text-warning">
                          {formatMutationWarning(warning, t)}
                        </p>
                      )) ?? null}
                      {item.warnings.includes('preservedConflictingCopy') ? (
                        <p className="mt-1 text-xs text-warning">{t('skills.updatePlan.preservedConflicts')}</p>
                      ) : null}
                      {mutation?.agentTargets.length ? (
                        <div className="mt-2 space-y-1">
                          {mutation.agentTargets.map((agentResult) => (
                            <div
                              key={`${item.skillIdentity.skillName}:${agentResult.targetId}`}
                              className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground"
                            >
                              <span className="font-medium text-foreground">
                                {agentDisplayNames?.get(agentResult.agentId) ?? agentResult.agentId}
                              </span>
                              <Badge variant="secondary" className="text-[11px]">
                                {t(`mutation.result.status.${agentResult.status}`)}
                              </Badge>
                              {agentResult.actualMode ? (
                                <Badge variant="outline" className="text-[11px]">
                                  {t(`mutation.result.modes.${agentResult.actualMode}`)}
                                </Badge>
                              ) : null}
                              {agentResult.fallbackReason ? (
                                <span>{formatFallbackReason(agentResult.fallbackReason, t)}</span>
                              ) : null}
                              {agentResult.error ? (
                                <span className="text-destructive" role="alert">
                                  {formatMutationError(agentResult.error, t)}
                                </span>
                              ) : null}
                            </div>
                          ))}
                        </div>
                      ) : null}
                      {mutation?.recovery ? <RecoveryActions recovery={mutation.recovery} /> : null}
                      </div>
                    );
                  })}
                </div>
              ) : null}
            </div>
          ) : null}
        </div>

        <DialogFooter className="pt-4 border-t border-border">
          {phase === 'previewError' ? (
            <>
              {!closeBlocked ? (
                <Button variant="outline" onClick={() => handleOpenChange(false)}>{t('common.cancel')}</Button>
              ) : null}
              <Button onClick={() => { void handlePreviewRetry(); }}>{t('common.retry')}</Button>
            </>
          ) : phase === 'result' ? (
            <>
              {retryableResults.length > 0 ? (
                <Button
                  variant="outline"
                  title={t('skills.updatePlan.retryFailed')}
                  disabled={writeBlocked}
                  onClick={() => {
                    void handleRetryFailed();
                  }}
                >
                  {t('skills.updatePlan.retryFailed')}
                </Button>
              ) : null}
              {!closeBlocked ? (
                <Button onClick={() => handleOpenChange(false)}>{t('common.close')}</Button>
              ) : null}
            </>
          ) : (
            <>
              {!closeBlocked ? (
                <Button variant="outline" onClick={() => handleOpenChange(false)}>
                  {t('common.cancel')}
                </Button>
              ) : null}
              <Button onClick={handleConfirm} disabled={writeBlocked || confirming || phase !== 'ready' || !canConfirm}>
                {t('skills.updatePlan.confirm')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
