import { useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2, CircleAlert, CircleStop, LoaderCircle } from 'lucide-react';
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
import { Progress } from '@/components/ui/progress';
import { Skeleton } from '@/components/ui/skeleton';
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
import type {
  AgentId,
  ContextRef,
  ErrorReport,
  ObservedEntryOwner,
  UpdateSkillPreview,
  UpdateSkillResult,
} from '@/bindings';
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

function ownerLabels(
  owners: ObservedEntryOwner[],
  names?: Map<AgentId, string>,
): string {
  return owners
    .map((owner) => names?.get(owner.agentId) ?? owner.displayName ?? owner.agentId)
    .join(' · ');
}

function PreviewSkillRow({
  skill,
  decisions,
  setConflictDecision,
  agentDisplayNames,
}: {
  skill: UpdateSkillPreview;
  decisions: Set<string>;
  setConflictDecision: (entryId: string, overwrite: boolean) => void;
  agentDisplayNames?: Map<AgentId, string>;
}) {
  const { t } = useTranslation();

  return (
    <div className="space-y-2.5 py-3 first:pt-0 last:pb-0">
      <div className="flex min-w-0 items-start justify-between gap-3">
        <p className="min-w-0 truncate text-sm font-semibold">{skill.skillName}</p>
        {skill.blockingReasons.length > 0 ? (
          <Badge variant="destructive" className="shrink-0 text-xs">
            {t('skills.updatePlan.blocked')}
          </Badge>
        ) : null}
      </div>

      <div className="space-y-1 text-xs text-muted-foreground">
        <p>{t('skills.updatePlan.sharedSkillAction')}</p>
        {skill.cleanCopyCount > 0 ? (
          <p>{t('skills.updatePlan.cleanCopiesAction', { count: skill.cleanCopyCount })}</p>
        ) : null}
        {skill.adapterTargets.length > 0 ? (
          <p>
            {t('skills.updatePlan.adapterTargetsAction', {
              agents: ownerLabels(skill.adapterTargets, agentDisplayNames),
            })}
          </p>
        ) : null}
      </div>

      {skill.overwritePrivateEntries.length > 0 ? (
        <div className="space-y-2 border-l-2 border-warning/40 pl-3">
          <p className="text-xs font-medium text-foreground">
            {t('skills.updatePlan.conflictingCopies')}
          </p>
          {skill.overwritePrivateEntries.map((entry) => (
            <label
              key={entry.entryId}
              className="flex min-w-0 cursor-pointer items-start gap-2.5 text-xs text-muted-foreground"
            >
              <Checkbox
                checked={decisions.has(entry.entryId)}
                aria-label={t('skills.updatePlan.overwritePrivateEntry')}
                onCheckedChange={(checked) => {
                  setConflictDecision(entry.entryId, checked === true);
                }}
              />
              <span className="min-w-0">
                <span className="block text-foreground">
                  {ownerLabels(entry.owners, agentDisplayNames)}
                </span>
                <span>{t('skills.updatePlan.preserveConflictDefault')}</span>
              </span>
            </label>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function ExecutionView({
  batch,
  phase,
  subject,
  current,
  total,
  cancelling,
}: {
  batch: boolean;
  phase: string;
  subject?: string | null;
  current?: number | null;
  total?: number | null;
  cancelling: boolean;
}) {
  const { t } = useTranslation();
  const progressValue = current != null && total != null && total > 0
    ? Math.min(100, (current / total) * 100)
    : null;

  return (
    <div
      className="flex min-h-52 flex-col items-center justify-center px-6 py-8 text-center"
      role="status"
      aria-live="polite"
    >
      <LoaderCircle className="h-8 w-8 animate-spin text-primary" aria-hidden="true" />
      <p className="mt-4 text-base font-semibold">
        {cancelling ? t('skills.updatePlan.stopping') : t(`mutation.phase.${phase}`)}
      </p>
      <p className="mt-1 max-w-md text-sm text-muted-foreground">
        {subject
          ? t('skills.updatePlan.currentSkill', { skillName: subject })
          : t('skills.updatePlan.executionDescription')}
      </p>
      {batch && progressValue != null ? (
        <div className="mt-5 w-full max-w-sm space-y-2">
          <Progress value={progressValue} className="h-2" />
          <p className="text-xs text-muted-foreground">
            {t('skills.updatePlan.progress', { current, total })}
          </p>
        </div>
      ) : null}
    </div>
  );
}

export function UpdatePlanDialog({
  open,
  context,
  skillNames,
  agentDisplayNames,
  onOpenChange,
}: UpdatePlanDialogProps) {
  const { t } = useTranslation();
  const phase = useSkillUpdateWorkflow((state) => state.phase);
  const preview = useSkillUpdateWorkflow((state) => state.preview);
  const result = useSkillUpdateWorkflow((state) => state.result);
  const executionError = useSkillUpdateWorkflow((state) => state.executionError);
  const decisions = useSkillUpdateWorkflow((state) => state.conflictDecisions);
  const setConflictDecision = useSkillUpdateWorkflow((state) => state.setConflictDecision);
  const confirmWorkflow = useSkillUpdateWorkflow((state) => state.confirm);
  const retryWorkflow = useSkillUpdateWorkflow((state) => state.retryFailed);
  const retryPreview = useSkillUpdateWorkflow((state) => state.open);
  const acceptMutation = useSkillUpdateWorkflow((state) => state.acceptMutation);
  const activeMutation = useMutationStore((state) => state.activeMutation);
  const cancelling = useMutationStore((state) => state.cancelling);
  const cancelActiveMutation = useMutationStore((state) => state.cancelActiveMutation);
  const environments = useEnvironmentStore((state) => state.environments);
  const projectsByEnvironment = useProjectStore((state) => state.projectsByEnvironment);
  const displayResults = result?.skills ?? EMPTY_RESULTS;
  const batch = skillNames.length > 1;
  const executing = phase === 'executing';
  const matchingUpdateMutation = context !== null && activeMutation?.kind === 'update'
    && contextKey(activeMutation.context) === contextKey(context);
  const writeBlocked = activeMutation !== null;

  useEffect(() => {
    acceptMutation(activeMutation);
  }, [acceptMutation, activeMutation]);

  const previewGroups = useMemo(() => {
    const groups = new Map<string, {
      sourceDisplay: string;
      refDisplay: string;
      skills: UpdateSkillPreview[];
    }>();
    for (const skill of preview?.skills ?? []) {
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
  }, [preview]);

  const sourceErrors = useMemo(
    () => new Map(result?.sources.map((source) => [source.id, source.error]) ?? []),
    [result],
  );
  const resultCounts = useMemo(() => ({
    success: displayResults.filter((item) => item.mutation?.status === 'succeeded').length,
    partial: displayResults.filter((item) => item.coverage.kind === 'preservedConflicts').length,
    failed: displayResults.filter((item) => (
      updateSkillStatus(item, sourceErrors.get(item.sourceResultId)) === 'failed'
    )).length,
    skipped: displayResults.filter((item) => (
      updateSkillStatus(item, sourceErrors.get(item.sourceResultId)) === 'cancelled'
    )).length,
  }), [displayResults, sourceErrors]);
  const retryableResults = useMemo(
    () => displayResults.filter((item) => item.retryable),
    [displayResults],
  );
  const resultPresentations = useMemo(
    () => new Map(displayResults.flatMap((item) => item.mutation ? [[
      item.skillIdentity.skillName,
      presentMutationUnit(item.mutation, t, { environments, projectsByEnvironment }),
    ] as const] : [])),
    [displayResults, environments, projectsByEnvironment, t],
  );
  const detailResults = useMemo(() => displayResults.filter((item) => (
    item.mutation?.status !== 'succeeded'
    || item.coverage.kind !== 'updated'
    || (item.mutation?.warnings.length ?? 0) > 0
    || item.warnings.length > 0
    || item.mutation?.recovery != null
  )), [displayResults]);

  if (!context) return null;

  const canConfirm = preview?.skills.some((skill) => (
    skill.capability.canRunUpdate && skill.blockingReasons.length === 0
  )) ?? false;
  const readyCount = preview?.skills.filter((skill) => (
    skill.capability.canRunUpdate && skill.blockingReasons.length === 0
  )).length ?? skillNames.length;
  const singleSkill = preview?.skills[0];
  const dialogTitle = phase === 'result'
    ? t('skills.updatePlan.resultTitle')
    : executing
      ? t(batch ? 'skills.updatePlan.executingBatchTitle' : 'skills.updatePlan.executingTitle', {
        skillName: singleSkill?.skillName ?? skillNames[0],
      })
      : batch
        ? t('skills.updatePlan.readyTitle', { count: readyCount })
        : t('skills.updatePlan.singleTitle', { skillName: singleSkill?.skillName ?? skillNames[0] });
  const dialogDescription = executing
    ? t('skills.updatePlan.executionDescription')
    : phase === 'result'
      ? t('skills.updatePlan.resultDescription')
      : t('skills.updatePlan.readyDescription');

  const handleDismiss = () => {
    if (!executing) onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (nextOpen) onOpenChange(true);
        else handleDismiss();
      }}
    >
      <DialogContent
        className={`${batch
          ? 'h-[min(40rem,calc(100dvh-2rem))] sm:max-w-2xl'
          : 'max-h-[min(32rem,calc(100dvh-2rem))] sm:max-w-xl'} grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0`}
        showCloseButton={!executing}
        aria-busy={phase === 'loadingPreview' || executing}
        onPointerDownOutside={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => {
          if (executing) event.preventDefault();
        }}
      >
        <DialogHeader className="border-b border-border px-6 pt-6 pb-4">
          <DialogTitle>{dialogTitle}</DialogTitle>
          <DialogDescription>{dialogDescription}</DialogDescription>
        </DialogHeader>

        <div
          data-testid="update-plan-dialog-body"
          className="min-h-0 overflow-y-auto overscroll-contain px-6 py-4"
        >
          {phase === 'loadingPreview' ? (
            <div className="min-h-48 space-y-3" role="status" aria-live="polite">
              <span className="sr-only">{t('skills.updatePlan.loadingPreview')}</span>
              <Skeleton className="h-12 w-full" />
              <Skeleton className="h-20 w-full" />
              <Skeleton className="h-12 w-4/5" />
            </div>
          ) : phase === 'previewError' ? (
            <div className="min-h-48 py-8 text-sm text-destructive" role="alert">
              {t('skills.updatePlan.previewError')}
            </div>
          ) : executing ? (
            <ExecutionView
              batch={batch}
              phase={matchingUpdateMutation ? activeMutation.phase : 'preparing'}
              subject={matchingUpdateMutation ? activeMutation.progress?.subject : null}
              current={matchingUpdateMutation ? activeMutation.progress?.current : null}
              total={matchingUpdateMutation ? activeMutation.progress?.total : null}
              cancelling={cancelling}
            />
          ) : phase === 'ready' && preview ? (
            <div className="space-y-4">
              {!batch && singleSkill ? (
                <>
                  <div className="flex items-start justify-between gap-4 border-b border-border pb-3">
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium">{singleSkill.sourceDisplay}</p>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t('skills.refBadge', { ref: singleSkill.refDisplay })}
                      </p>
                    </div>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {t('skills.updatePlan.source')}
                    </span>
                  </div>
                  <PreviewSkillRow
                    skill={singleSkill}
                    decisions={decisions}
                    setConflictDecision={setConflictDecision}
                    agentDisplayNames={agentDisplayNames}
                  />
                </>
              ) : (
                previewGroups.map((group) => (
                  <section key={`${group.sourceDisplay}:${group.refDisplay}`}>
                    <div className="flex items-end justify-between gap-3 border-b border-border pb-2">
                      <div className="min-w-0">
                        <h3 className="truncate text-sm font-semibold">{group.sourceDisplay}</h3>
                        <p className="mt-0.5 text-xs text-muted-foreground">
                          {t('skills.refBadge', { ref: group.refDisplay })}
                        </p>
                      </div>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {t('skills.updatePlan.skillCount', { count: group.skills.length })}
                      </span>
                    </div>
                    <div className="divide-y divide-border/70">
                      {group.skills.map((skill) => (
                        <PreviewSkillRow
                          key={skill.skillName}
                          skill={skill}
                          decisions={decisions}
                          setConflictDecision={setConflictDecision}
                          agentDisplayNames={agentDisplayNames}
                        />
                      ))}
                    </div>
                  </section>
                ))
              )}
            </div>
          ) : phase === 'result' ? (
            <div className="min-h-48 space-y-4" role="status" aria-live="polite">
              <div className="flex items-start gap-3">
                {result?.outcome === 'succeeded' ? (
                  <CheckCircle2 className="mt-0.5 h-5 w-5 text-success" aria-hidden="true" />
                ) : result?.outcome === 'cancelled' ? (
                  <CircleStop className="mt-0.5 h-5 w-5 text-warning" aria-hidden="true" />
                ) : (
                  <CircleAlert className="mt-0.5 h-5 w-5 text-warning" aria-hidden="true" />
                )}
                <div>
                  <p className="text-sm font-semibold">
                    {result
                      ? t(`skills.updatePlan.resultOutcome.${result.outcome}`)
                      : t('skills.updatePlan.resultOutcome.failed')}
                  </p>
                  {executionError ? (
                    <p className="mt-1 text-sm text-destructive" role="alert">
                      {formatAppError(executionError, t)}
                    </p>
                  ) : (
                    <p className="mt-1 text-xs text-muted-foreground">
                      {t('skills.updatePlan.resultSummary', resultCounts)}
                    </p>
                  )}
                </div>
              </div>

              {result?.sources.filter((source) => source.error).map((source) => (
                <div key={source.id} className="border-t border-border pt-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium">{source.source}</span>
                    <Badge variant="outline" className="text-xs">
                      {t(`mutation.result.status.${isCancelled(source.error) ? 'cancelled' : source.status}`)}
                    </Badge>
                  </div>
                  {source.error ? (
                    <p className="mt-1 text-xs text-destructive" role="alert">
                      {formatMutationError(source.error, t)}
                    </p>
                  ) : null}
                </div>
              ))}

              {detailResults.map((item) => {
                const mutation = item.mutation;
                const presentation = resultPresentations.get(item.skillIdentity.skillName);
                const source = result?.sources.find((entry) => entry.id === item.sourceResultId);
                const coverageError = item.coverage.kind === 'notUpdated'
                  ? item.coverage.error
                  : null;
                const error = mutation?.error
                  ?? (source?.error?.code === coverageError?.code ? null : coverageError);
                const status = updateSkillStatus(item, source?.error);
                return (
                  <div key={item.skillIdentity.skillName} className="border-t border-border pt-3">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-sm font-medium">{item.skillIdentity.skillName}</span>
                      <Badge variant="outline" className="text-xs">
                        {t(`mutation.result.status.${status}`)}
                      </Badge>
                      {source ? (
                        <span className="text-xs text-muted-foreground">{source.source}</span>
                      ) : null}
                    </div>
                    {presentation ? (
                      <p className="mt-1 text-xs text-muted-foreground">
                        {t('mutation.result.location', {
                          environment: presentation.environmentLabel,
                          scope: presentation.scopeLabel,
                        })}
                      </p>
                    ) : null}
                    {error ? (
                      <p className="mt-1 text-xs text-destructive" role="alert">
                        {formatMutationError(error, t)}
                      </p>
                    ) : null}
                    {mutation?.fallbackReason ? (
                      <p className="mt-1 text-xs text-muted-foreground">
                        {formatFallbackReason(mutation.fallbackReason, t)}
                      </p>
                    ) : null}
                    {mutation?.warnings.map((warning, index) => (
                      <p key={`${item.skillIdentity.skillName}:warning:${warning.code}:${index}`} className="mt-1 text-xs text-warning">
                        {formatMutationWarning(warning, t)}
                      </p>
                    )) ?? null}
                    {item.warnings.includes('preservedConflictingCopy') ? (
                      <p className="mt-1 text-xs text-warning">
                        {t('skills.updatePlan.preservedConflicts')}
                      </p>
                    ) : null}
                    {mutation?.recovery ? <RecoveryActions recovery={mutation.recovery} /> : null}
                  </div>
                );
              })}
            </div>
          ) : null}
        </div>

        <DialogFooter className="min-h-17 border-t border-border px-6 py-4">
          {phase === 'previewError' ? (
            <>
              <Button variant="outline" onClick={handleDismiss}>{t('common.cancel')}</Button>
              <Button onClick={() => { void retryPreview(context, skillNames); }}>
                {t('common.retry')}
              </Button>
            </>
          ) : phase === 'ready' ? (
            <>
              <Button variant="outline" onClick={handleDismiss}>{t('common.cancel')}</Button>
              <Button
                onClick={() => { void confirmWorkflow(); }}
                disabled={writeBlocked || !canConfirm}
              >
                {t('skills.updatePlan.confirm')}
              </Button>
            </>
          ) : executing ? (
            matchingUpdateMutation && activeMutation.cancelable ? (
              <Button
                variant="outline"
                className="text-destructive hover:text-destructive"
                disabled={cancelling}
                onClick={() => { void cancelActiveMutation(); }}
              >
                {cancelling ? (
                  <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden="true" />
                ) : (
                  <CircleStop className="h-4 w-4" aria-hidden="true" />
                )}
                {cancelling ? t('skills.updatePlan.stopping') : t('skills.updatePlan.stop')}
              </Button>
            ) : (
              <p className="text-sm text-muted-foreground">
                {t(matchingUpdateMutation
                  ? 'skills.updatePlan.finishing'
                  : 'skills.updatePlan.starting')}
              </p>
            )
          ) : phase === 'result' ? (
            <>
              {retryableResults.length > 0 ? (
                <Button
                  variant="outline"
                  disabled={writeBlocked}
                  onClick={() => { void retryWorkflow(); }}
                >
                  {t('skills.updatePlan.retryFailed')}
                </Button>
              ) : null}
              <Button onClick={handleDismiss}>{t('common.close')}</Button>
            </>
          ) : (
            <Button variant="outline" onClick={handleDismiss}>{t('common.cancel')}</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
