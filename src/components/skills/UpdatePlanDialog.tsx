import { useEffect, useId, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2, CircleAlert, CircleStop, Files, Link2, LoaderCircle } from 'lucide-react';
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
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { Progress } from '@/components/ui/progress';
import { Skeleton } from '@/components/ui/skeleton';
import { canConfirmSkillUpdate, canPrepareUpdateAgain, useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { useMutationStore } from '@/stores/mutation';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import {
  formatFallbackReason,
  formatMutationError,
  formatMutationWarning,
} from '@/lib/mutation-results';
import type {
  AgentId,
  SkillLocationRef,
  ScopePathBase,
  ErrorReport,
  ObservedEntryReader,
  UpdateSkillPreview,
  UpdateSkillResult,
  UpdateTargetPreview,
  UpdateSourcePreview,
  ResourceLocator,
} from '@/bindings';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { contextKey } from '@/lib/context';
import { formatAppError } from '@/utils/format-app-error';
import { toAppError } from '@/utils/to-app-error';
import { projectDisplayName } from '@/lib/projects/presentation';
import './UpdatePlanDialog.css';
import { presentInstallPath } from '@/lib/install-path';

const EMPTY_RESULTS: UpdateSkillResult[] = [];
const NATIVE_ENVIRONMENT = { kind: 'native' as const };

interface UpdatePlanDialogProps {
  open: boolean;
  context: SkillLocationRef | null;
  skillNames: string[];
  agentDisplayNames?: Map<AgentId, string>;
  onOpenChange: (open: boolean) => void;
}

function isCancelled(report: ErrorReport | null | undefined): boolean {
  return report?.code === 'mutationCancelled';
}

function updateSkillStatus(item: UpdateSkillResult, sourceError?: ErrorReport | null): string {
  if (item.mutation) return item.mutation.status;
  if (item.coverage.kind === 'notUpdated' && item.coverage.error.code === 'noUpdateTargets') return 'notRun';
  if (
    (item.coverage.kind === 'notUpdated' && isCancelled(item.coverage.error))
    || isCancelled(sourceError)
  ) {
    return 'cancelled';
  }
  return 'failed';
}

function ownerLabels(
  readers: ObservedEntryReader[],
  names?: Map<AgentId, string>,
): string {
  return readers
    .map((reader) => reader.displayName || names?.get(reader.agentId) || reader.agentId)
    .join(' · ');
}

function UpdatePath({ path, pathBase, scope }: {
  path: ResourceLocator;
  pathBase?: ScopePathBase | null;
  scope: SkillLocationRef['scope']['scope'];
}) {
  const label = presentInstallPath(path, pathBase, scope).label;
  return <Tooltip>
    <TooltipTrigger asChild>
      <span tabIndex={0} className="update-position-path rounded-sm font-mono text-xs text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" translate="no">{label}</span>
    </TooltipTrigger>
    <TooltipContent side="top" align="center" sideOffset={6} collisionPadding={12} className="max-w-[min(32rem,calc(100vw-3rem))] break-all text-wrap">
      <code translate="no">{path.nativePath}</code>
    </TooltipContent>
  </Tooltip>;
}

function LocationRow({ skillName, path, readers, isStandard, status, kind, checked, selection,
  agentDisplayNames, pathBase, scope }: {
  skillName: string;
  path: ResourceLocator;
  readers: ObservedEntryReader[];
  isStandard?: boolean | null;
  status?: 'different' | 'restore' | 'preserved';
  kind?: 'copy' | 'link';
  checked: boolean;
  selection?: { change: (checked: boolean) => void };
  agentDisplayNames?: Map<AgentId, string>;
  pathBase?: ScopePathBase | null;
  scope: SkillLocationRef['scope']['scope'];
}) {
  const { t } = useTranslation();
  const id = useId();
  const names = [...new Set(readers.map((reader) => ownerLabels([reader], agentDisplayNames)))];
  const label = isStandard ? t('skills.updatePlan.commonDirectory') : names.join(' · ') || t('skills.updatePlan.installLocation');
  const ShapeIcon = kind === 'link' ? Link2 : Files;
  return (
    <li className="update-position-row">
      <Checkbox id={id} checked={checked} disabled={!selection}
        aria-label={`${skillName} · ${label} · ${presentInstallPath(path, pathBase, scope).label}`}
        onCheckedChange={(value) => selection?.change(value === true)} />
      <label htmlFor={id} className={`update-position-name ${selection ? 'cursor-pointer' : ''}`}>{label}</label>
      <UpdatePath path={path} pathBase={pathBase} scope={scope} />
      <span className={`update-position-status ${status === 'different' ? 'text-warning' : 'text-muted-foreground'}`}>
        {status ? t(status === 'different' ? 'skills.updatePlan.contentDifferent' : status === 'restore' ? 'skills.updatePlan.restoreLocation' : 'skills.updatePlan.excludedLocation') : null}
      </span>
      <span className="update-position-mode text-muted-foreground">
        {kind && !isStandard && status !== 'restore' ? <><ShapeIcon className="size-3.5 shrink-0" aria-hidden="true" />
          {t(kind === 'link' ? 'skills.updatePlan.linkKind' : 'skills.updatePlan.copyKind')}</> : null}
      </span>
    </li>
  );
}

function PreviewSkillRow({ skill, decisions, setCopySelected, agentDisplayNames, pathBase, scope }: {
  skill: UpdateSkillPreview;
  decisions: Set<string>;
  setCopySelected: (entryId: string, overwrite: boolean) => void;
  agentDisplayNames?: Map<AgentId, string>;
  pathBase?: ScopePathBase | null;
  scope: SkillLocationRef['scope']['scope'];
}) {
  const { t } = useTranslation();
  const id = useId();
  const rowProps = { skillName: skill.skillName, agentDisplayNames, pathBase, scope };
  const targetRow = (target: UpdateTargetPreview, preserved = false) => {
    const entryId = preserved ? null : target.selectableEntryId;
    return <LocationRow key={target.displayPath.nativePath} {...rowProps} path={target.displayPath} readers={target.readers}
      isStandard={target.isStandard} status={preserved ? 'preserved' : target.restoring ? 'restore' : undefined}
      kind={target.restoring ? undefined : target.kind === 'symlink' || target.kind === 'junction' ? 'link' : target.kind === 'directory' ? 'copy' : undefined}
      checked={!preserved && (!entryId || decisions.has(entryId))}
      selection={entryId ? { change: (checked) => setCopySelected(entryId, checked) } : undefined} />;
  };
  return (
    <section className="update-skill-section" aria-labelledby={`${id}-title`}>
      <h3 id={`${id}-title`} className="update-skill-heading">{skill.skillName}</h3>
      {skill.blockingReasons.includes('noUpdateTargets') ? <p className="mb-2 text-xs text-muted-foreground">{t('skills.updatePlan.noUpdateLocations')}</p> : null}
      {skill.blockingReasons.filter((reason) => reason !== 'noUpdateTargets').map((reason) =>
        <p key={reason} className="text-xs text-destructive" role="alert">{formatMutationError({ code: reason, parameters: {}, field: null, severity: 'error', retryable: false, technicalDetails: null, environment: null, context: null, unitId: null, recoveryResourceId: null, displayPaths: [] }, t)}</p>)}
      <ul className="update-skill-positions" aria-label={t('skills.updatePlan.updateLocations')}>
        {skill.targets.filter((target) => target.isStandard).map((target) => targetRow(target))}
        {skill.overwritePrivateEntries.map((entry) => <LocationRow key={entry.entryId} {...rowProps}
          path={entry.displayPath} readers={entry.readers} isStandard={entry.isStandard}
          status="different" kind="copy" checked={decisions.has(entry.entryId)}
          selection={{ change: (checked) => setCopySelected(entry.entryId, checked) }} />)}
        {skill.targets.filter((target) => !target.isStandard).map((target) => targetRow(target))}
        {skill.linkedTargets.map((target) => <LocationRow key={target.displayPath.nativePath} {...rowProps}
          path={target.displayPath} readers={target.readers} isStandard={target.isStandard} kind="link"
          checked={!target.targetCopyEntryId || decisions.has(target.targetCopyEntryId)} />)}
        {skill.preservedTargets?.map((target) => targetRow(target, true))}
      </ul>
    </section>
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
  const previewError = useSkillUpdateWorkflow((state) => state.previewError);
  const result = useSkillUpdateWorkflow((state) => state.result);
  const executionError = useSkillUpdateWorkflow((state) => state.executionError);
  const decisions = useSkillUpdateWorkflow((state) => state.selectedCopyEntries);
  const setCopySelected = useSkillUpdateWorkflow((state) => state.setCopySelected);
  const confirmWorkflow = useSkillUpdateWorkflow((state) => state.confirm);
  const retryWorkflow = useSkillUpdateWorkflow((state) => state.retryFailed);
  const retryPreview = useSkillUpdateWorkflow((state) => state.open);
  const acceptMutation = useSkillUpdateWorkflow((state) => state.acceptMutation);
  const activeMutation = useMutationStore((state) => state.activeMutation);
  const cancelling = useMutationStore((state) => state.cancelling);
  const cancelActiveMutation = useMutationStore((state) => state.cancelActiveMutation);
  const businessWriteBlocked = useBusinessWriteBlocked();
  const projectEnvironment = context?.environment ?? NATIVE_ENVIRONMENT;
  const { projects } = useProjectWorkspace(projectEnvironment);
  const displayResults = result?.skills ?? EMPTY_RESULTS;
  const batch = skillNames.length > 1;
  const executing = phase === 'executing';
  const matchingUpdateMutation = context !== null && activeMutation?.kind === 'update'
    && activeMutation.target.kind === 'skillLocation'
    && contextKey({
      environment: activeMutation.target.environment,
      scope: activeMutation.target.scope,
    }) === contextKey(context);
  const writeBlocked = businessWriteBlocked;

  useEffect(() => {
    acceptMutation(activeMutation);
  }, [acceptMutation, activeMutation]);

  const previewGroups = useMemo(() => {
    const groups: UpdateSourcePreview[] = (preview?.sources ?? []).map((group) => ({ ...group, skillNames: [...group.skillNames] }));
    const known = new Set(groups.flatMap((group) => group.skillNames));
    for (const skill of preview?.skills ?? []) {
      if (known.has(skill.skillName)) continue;
      const key = skill.sourceKey ?? skill.skillName;
      let group = groups.find((candidate) => candidate.sourceKey === key);
      if (!group) {
        group = { sourceKey: key, sourceDisplay: skill.sourceDisplay, refDisplay: skill.refDisplay, skillNames: [], error: null };
        groups.push(group);
      }
      group.skillNames.push(skill.skillName);
    }
    return groups;
  }, [preview]);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);

  const sourceErrors = useMemo(
    () => new Map(result?.sources.map((source) => [source.id, source.error]) ?? []),
    [result],
  );
  const resultCounts = useMemo(() => ({
    success: displayResults.filter((item) => item.mutation?.status === 'succeeded' && item.coverage.kind === 'updated').length,
    partial: displayResults.filter((item) => item.coverage.kind === 'updatedWithSkippedCopies').length,
    failed: displayResults.filter((item) => (
      updateSkillStatus(item, sourceErrors.get(item.sourceResultId)) === 'failed'
    )).length,
    skipped: displayResults.filter((item) => (
      ['cancelled', 'notRun'].includes(updateSkillStatus(item, sourceErrors.get(item.sourceResultId)))
    )).length,
  }), [displayResults, sourceErrors]);
  const retryableResults = useMemo(
    () => displayResults.filter((item) => item.retryable),
    [displayResults],
  );
  const canPrepareAgain = canPrepareUpdateAgain(executionError);
  const detailResults = displayResults;

  if (!context) return null;

  const canConfirm = preview?.skills.some((skill) => (
    !preview.blocked.some((issue) => issue.skillName === skill.skillName) && canConfirmSkillUpdate(skill, decisions)
  )) ?? false;
  const scope = context.scope;
  const project = scope.scope === 'project' ? projects.find((item) => item.binding.id === scope.project_id) : undefined;
  const subjectTitle = scope.scope === 'global' ? t('skills.updatePlan.globalTitle')
    : t('skills.updatePlan.projectTitle', { project: project ? projectDisplayName(project) : scope.project_id });
  const dialogTitle = phase === 'result' ? t('skills.updatePlan.resultTitle') : subjectTitle;
  const hasCopies = phase === 'ready' && preview?.skills.some((skill) => skill.overwritePrivateEntries.length > 0 || skill.targets.some((target) => target.selectableEntryId));
  const onlyPreserved = phase === 'ready' && !!preview?.skills.length && !preview.blocked.length
    && preview.skills.every((skill) => skill.blockingReasons.includes('noUpdateTargets'));
  const groupedNames = new Set(previewGroups.flatMap((group) => group.skillNames));
  const ungroupedIssues = preview?.blocked.filter((issue) => !groupedNames.has(issue.skillName)) ?? [];

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
        className="update-plan-dialog max-h-[calc(100dvh-3rem)] grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0"
        onOpenAutoFocus={(event) => {
          returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
          event.preventDefault();
          cancelButton.current?.focus();
        }}
        onCloseAutoFocus={(event) => {
          if (returnFocus.current?.isConnected) {
            event.preventDefault();
            returnFocus.current.focus();
          }
        }}
        dismissible={!executing}
        closeLabel={t('common.close')}
        aria-busy={phase === 'loadingPreview' || executing}
      >
        <DialogHeader className="min-h-14 flex-row items-center gap-4 border-b border-border py-3 pl-5 pr-12 text-left">
          <DialogTitle className="min-w-0 truncate text-base">{dialogTitle}</DialogTitle>
          <DialogDescription className="sr-only">{t('skills.updatePlan.readyDescription')}</DialogDescription>
        </DialogHeader>

        <div
          data-testid="update-plan-dialog-body"
          className="min-h-0 overflow-y-auto overscroll-contain px-5 py-3"
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
              {previewError ? formatAppError(toAppError(previewError), t) : t('skills.updatePlan.previewError')}
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
            <TooltipProvider>
            <div>
              {preview.redirectedDownloadHosts.length > 0 ? <p className="text-sm text-warning">{t('libraries.redirectConfirmation', { host: preview.redirectedDownloadHosts.join(', ') })}</p> : null}
              {executionError ? <p className="text-sm text-warning" role="alert">{formatAppError(executionError, t)}</p> : null}
              {previewGroups.map((group) => (
                <section key={group.sourceKey} className="update-source-group">
                  <p className="update-source-heading">{t('skills.updatePlan.source')} {group.sourceDisplay}{group.refDisplay ? <> · {group.refDisplay}</> : null}</p>
                  {group.error ? <p className="text-xs text-destructive" role="alert">{formatAppError(group.error, t)}</p> : null}
                  <div className="update-source-skills">
                    {group.skillNames.map((name) => {
                      const skill = preview.skills.find((skill) => skill.skillName === name);
                      const issue = preview.blocked.find((issue) => issue.skillName === name);
                      return skill && !issue ? <PreviewSkillRow key={name} skill={skill} decisions={decisions}
                        setCopySelected={setCopySelected} agentDisplayNames={agentDisplayNames}
                        pathBase={preview.pathBase} scope={context.scope.scope} />
                        : <section key={name} className="space-y-1">
                          <div className="flex items-start justify-between gap-3"><h3 className="min-w-0 break-words text-sm font-semibold">{name}</h3>
                            <span className="shrink-0 text-xs text-destructive">{t('skills.updatePlan.blocked')}</span></div>
                          {issue && !group.error ? <p className="text-xs text-destructive" role="alert">{formatAppError(issue.error, t)}</p> : null}
                        </section>;
                    })}
                  </div>
                </section>
              ))}
              {ungroupedIssues.map((issue) => <section key={issue.skillName} className="space-y-1">
                <h3 className="text-sm font-semibold">{issue.skillName}</h3>
                <p className="text-xs text-destructive" role="alert">{formatAppError(issue.error, t)}</p>
              </section>)}
            </div>
            </TooltipProvider>
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
                        {t(item.coverage.kind === 'updatedWithSkippedCopies' ? 'skills.updatePlan.updatedWithSkippedCopies' : `mutation.result.status.${status}`)}
                      </Badge>
                      {source ? (
                        <span className="text-xs text-muted-foreground">{source.source}</span>
                      ) : null}
                    </div>
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
                    {item.skippedCopyPaths?.length ? (
                      <div className="mt-2 space-y-1 text-xs text-muted-foreground">
                        <p>{t('skills.updatePlan.skippedCopies')}</p>
                        {item.skippedCopyPaths.map((path) => <p key={path.nativePath} className="min-w-0">
                          <span className="block truncate font-mono" title={path.nativePath}>{presentInstallPath(path, preview?.pathBase, context.scope.scope).label}</span>
                        </p>)}
                      </div>
                    ) : null}
                    {preview?.skills.find((skill) => skill.skillName === item.skillIdentity.skillName)?.preservedTargets?.length ? (
                      <p className="mt-1 text-xs text-muted-foreground">{t('skills.updatePlan.preservedReferences')}</p>
                    ) : null}
                    {mutation?.recovery ? <RecoveryActions recovery={mutation.recovery} /> : null}
                  </div>
                );
              })}
            </div>
          ) : null}
        </div>

        <DialogFooter className="min-h-14 flex-row items-center gap-3 border-t border-border px-5 py-2.5">
          {phase === 'previewError' ? (
            <>
              <Button ref={cancelButton} variant="outline" onClick={handleDismiss}>{t('common.cancel')}</Button>
              <Button onClick={() => { void retryPreview(context, skillNames); }}>
                {t('common.retry')}
              </Button>
            </>
          ) : phase === 'ready' ? (
            onlyPreserved ? <Button ref={cancelButton} onClick={handleDismiss}>{t('common.close')}</Button> : <>
              {hasCopies || preview?.blocked.length ? <div className="mr-auto min-w-0 flex-1 space-y-0.5 text-xs text-muted-foreground">
                {hasCopies ? <p>{t('skills.updatePlan.preserveConflictDefault')}</p> : null}
                {preview?.blocked.length ? <p>{t('skills.updatePlan.blockedCount', { count: preview.blocked.length })}</p> : null}
              </div> : null}
              <Button ref={cancelButton} variant="outline" onClick={handleDismiss}>{t('common.cancel')}</Button>
              {!canConfirm && preview?.blocked.length ? <Button variant="outline" onClick={() => { void retryPreview(context, skillNames); }}>{t('common.retry')}</Button> : null}
              {canConfirm || !preview?.blocked.length ? <Button onClick={() => { void confirmWorkflow(); }} disabled={writeBlocked || !canConfirm}>
                {t('skills.updatePlan.confirm')}
              </Button> : null}
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
              {retryableResults.length > 0 || canPrepareAgain ? (
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
            <Button ref={cancelButton} variant="outline" onClick={handleDismiss}>{t('common.cancel')}</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
