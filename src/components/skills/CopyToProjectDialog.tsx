// src/components/skills/CopyToProjectDialog.tsx
import { useState, useCallback, useMemo, useEffect, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Folder, Loader2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Skeleton } from '@/components/ui/skeleton';
import { environmentKey } from '@/lib/context';
import { formatMutationError } from '@/lib/mutation-results';
import type {
  AgentSelectionSubmission,
  CopyAgentSelectionSnapshot,
  SkillLocationRef,
  EnvironmentInfo,
  EnvironmentRef,
  InstalledSkill,
  ProjectInfo,
} from '@/bindings';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { ProjectIdentity } from '@/components/projects/ProjectIdentity';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import type { CopyOutcome } from '@/workflows/skill-copy';
import { AgentSelectionPanel } from '@/components/agents/selection/AgentSelectionPanel';
import {
  useAgentSelectionSession,
  type CopyAgentSelectionSessionRequest,
} from '@/hooks/useAgentSelectionSession';
import { getCopyableProjects } from '@/lib/projects/copy-targets';

export interface CopyTargetSelection {
  environment: EnvironmentRef;
  projectIds: string[];
  agentSelection: AgentSelectionSubmission;
}

interface CopyToProjectDialogProps {
  open?: boolean;
  skill: InstalledSkill | null;
  sourceContext: SkillLocationRef;
  environments: EnvironmentInfo[];
  projectsByEnvironment: Record<string, ProjectInfo[]>;
  loadAgentSelection: (
    request: CopyAgentSelectionSessionRequest,
  ) => Promise<CopyAgentSelectionSnapshot>;
  onLoadProjects: (environment: EnvironmentRef) => Promise<void>;
  /** 检查 skill 在目标项目中是否已存在 */
  checkExistence?: (
    skillName: string,
    environment: EnvironmentRef,
    projectIds: string[],
  ) => Promise<Array<{ projectId: string; hasSkill: boolean }>>;
  onClose: () => void;
  onCopy: (selection: CopyTargetSelection) => Promise<CopyOutcome>;
}

type ProjectLoadState = 'idle' | 'loading' | 'ready' | 'error';
type ProjectLoadFailure = 'environment' | 'catalog' | null;
type PresenceState = 'idle' | 'loading' | 'ready' | 'error';
type ProjectPresence = 'installed' | 'absent' | 'unknown';
type TargetEnvironmentSelection =
  | { kind: 'valid'; key: string }
  | { kind: 'missing'; key: string };

function classifyProjectLoadFailure(error: unknown): Exclude<ProjectLoadFailure, null> {
  return error != null
    && typeof error === 'object'
    && 'failureSource' in error
    && error.failureSource === 'environment'
    ? 'environment'
    : 'catalog';
}

export const CopyToProjectDialog = memo(function CopyToProjectDialog({
  open = true,
  skill,
  sourceContext,
  ...sessionProps
}: CopyToProjectDialogProps) {
  const scopeKey = sourceContext.scope.scope === 'project'
    ? `project:${sourceContext.scope.project_id}`
    : 'global';
  const sessionKey = skill
    ? `${environmentKey(sourceContext.environment)}:${scopeKey}:${skill.canonicalPath}`
    : 'closed';

  return (
    <CopyToProjectDialogSession
      key={sessionKey}
      open={open}
      skill={skill}
      sourceContext={sourceContext}
      {...sessionProps}
    />
  );
});

function CopyToProjectDialogSession({
  open = true,
  skill,
  sourceContext,
  environments,
  projectsByEnvironment,
  onLoadProjects,
  checkExistence,
  loadAgentSelection,
  onClose,
  onCopy,
}: CopyToProjectDialogProps) {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const [copying, setCopying] = useState(false);
  const [projectLoadState, setProjectLoadState] = useState<ProjectLoadState>('idle');
  const [projectLoadFailure, setProjectLoadFailure] = useState<ProjectLoadFailure>(null);
  const [projectLoadAttempt, setProjectLoadAttempt] = useState(0);
  const [presenceState, setPresenceState] = useState<PresenceState>('idle');
  const [targetEnvironmentSelection, setTargetEnvironmentSelection] = useState<
    TargetEnvironmentSelection
  >(() => {
    const key = environmentKey(sourceContext.environment);
    return environments.some((entry) => environmentKey(entry.environment) === key)
      ? { kind: 'valid', key }
      : { kind: 'missing', key };
  });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [presenceByProject, setPresenceByProject] = useState<Map<string, ProjectPresence>>(
    () => new Map(),
  );
  const [completedProjectIds, setCompletedProjectIds] = useState<Set<string>>(new Set());
  const [copyOutcome, setCopyOutcome] = useState<CopyOutcome | null>(null);
  const agentSelectionRequest = useMemo<CopyAgentSelectionSessionRequest>(() => ({
    kind: 'copy',
    context: sourceContext,
    skillName: skill?.name ?? '',
  }), [skill?.name, sourceContext]);
  const agentSelection = useAgentSelectionSession({
    active: open && skill !== null,
    request: agentSelectionRequest,
    load: loadAgentSelection,
  });

  const clearTargetProjectState = useCallback(() => {
    setSelected(new Set());
    setCompletedProjectIds(new Set());
    setCopyOutcome(null);
    setProjectLoadState('idle');
    setProjectLoadFailure(null);
    setPresenceState('idle');
    setPresenceByProject(new Map());
  }, []);

  const targetEnvironmentEntry = targetEnvironmentSelection.kind === 'valid'
    ? environments.find(
      (entry) => environmentKey(entry.environment) === targetEnvironmentSelection.key,
    )
    : undefined;
  const targetEnvironment = targetEnvironmentEntry?.environment ?? null;
  const targetEnvironmentKey = targetEnvironmentSelection.key;

  useEffect(() => {
    if (targetEnvironmentSelection.kind !== 'valid' || targetEnvironmentEntry) return;
    setTargetEnvironmentSelection((current) => (
      current.kind === 'valid' && current.key === targetEnvironmentSelection.key
        ? { kind: 'missing', key: current.key }
        : current
    ));
    clearTargetProjectState();
  }, [clearTargetProjectState, targetEnvironmentEntry, targetEnvironmentSelection]);

  const availableProjects = useMemo(
    () => targetEnvironment ? getCopyableProjects({
      targetEnvironment,
      sourceContext,
      projects: projectsByEnvironment[targetEnvironmentKey] ?? [],
      completedProjectIds,
    }) : [],
    [completedProjectIds, projectsByEnvironment, sourceContext, targetEnvironment, targetEnvironmentKey],
  );

  useEffect(() => {
    if (!skill || !targetEnvironment) return;
    let cancelled = false;
    setProjectLoadState('loading');
    setProjectLoadFailure(null);
    setPresenceState('idle');
    setPresenceByProject(new Map());
    void onLoadProjects(targetEnvironment)
      .then(() => {
        if (!cancelled) setProjectLoadState('ready');
      })
      .catch((error) => {
        if (!cancelled) {
          setProjectLoadFailure(classifyProjectLoadFailure(error));
          setProjectLoadState('error');
        }
      });
    return () => { cancelled = true; };
  }, [onLoadProjects, projectLoadAttempt, skill, targetEnvironment]);

  useEffect(() => {
    if (!skill || !targetEnvironment || projectLoadState !== 'ready') return;
    if (!checkExistence || availableProjects.length === 0) {
      setPresenceState('idle');
      setPresenceByProject(new Map());
      return;
    }
    let cancelled = false;
    setPresenceState('loading');
    setPresenceByProject(new Map(
      availableProjects.map((project) => [project.binding.id, 'unknown' as const]),
    ));
    void checkExistence(
      skill.name,
      targetEnvironment,
      availableProjects.map((project) => project.binding.id),
    ).then((statuses) => {
      if (cancelled) return;
      const statusById = new Map(statuses.map((status) => [status.projectId, status.hasSkill]));
      setPresenceByProject(new Map(availableProjects.map((project) => {
        const installed = statusById.get(project.binding.id);
        return [
          project.binding.id,
          installed === undefined ? 'unknown' : installed ? 'installed' : 'absent',
        ];
      })));
      setPresenceState('ready');
    }).catch(() => {
      if (!cancelled) setPresenceState('error');
    });
    return () => { cancelled = true; };
  }, [availableProjects, checkExistence, projectLoadState, skill, targetEnvironment]);

  // 选中的项目中有多少个已存在此 skill
  const selectedExistingCount = useMemo(() => {
    let count = 0;
    for (const path of selected) {
      if (presenceByProject.get(path) === 'installed') count++;
    }
    return count;
  }, [presenceByProject, selected]);

  const hasWorkflowFeedback = copyOutcome !== null;
  const failedUnitByProjectId = useMemo(() => {
    const failedUnits = copyOutcome?.status === 'partial'
      ? copyOutcome.response.units.filter((unit) => unit.status !== 'succeeded')
      : copyOutcome?.status === 'failed' && copyOutcome.unit
        ? [copyOutcome.unit]
        : [];
    return new Map(failedUnits.flatMap((unit) => {
      if (unit.target.scope.scope !== 'project') return [];
      return [[unit.target.scope.project_id, unit] as const];
    }));
  }, [copyOutcome]);

  const toggleProject = useCallback((projectId: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(projectId)) {
        next.delete(projectId);
      } else {
        next.add(projectId);
      }
      return next;
    });
  }, []);

  const handleCopy = useCallback(async () => {
    if (agentSelection.status !== 'ready' || !targetEnvironment) return;
    setCopying(true);
    setCopyOutcome(null);
    try {
      const outcome = await onCopy({
        environment: targetEnvironment,
        projectIds: Array.from(selected),
        agentSelection: agentSelection.submission,
      });
      if (!outcome || outcome.status === 'succeeded') {
        onClose();
        return;
      }
      setCopyOutcome(outcome);
      if (outcome.status === 'selectionStale') {
        agentSelection.acceptSnapshot(outcome.snapshot);
        setCopyOutcome(null);
      } else if (outcome.status === 'partial') {
        setCompletedProjectIds((previous) => new Set([
          ...previous,
          ...outcome.succeededProjectIds,
        ]));
        setSelected(new Set(outcome.retryableProjectIds));
      } else if (outcome.status === 'recoveryRequired') {
        setSelected(new Set());
      } else if (outcome.status === 'failed' && outcome.unit && !outcome.unit.retryable) {
        setSelected(new Set());
      }
    } finally {
      setCopying(false);
    }
  }, [agentSelection, onClose, onCopy, selected, targetEnvironment]);

  return (
    <Dialog
      open={open && !!skill}
      onOpenChange={(nextOpen) => !nextOpen && open && !copying && onClose()}
    >
      <DialogContent
        className="grid h-[min(42rem,calc(100dvh-2rem))] w-[calc(100vw-2rem)] min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-5xl"
        dismissible={!copying}
        closeLabel={t('common.close')}
        aria-busy={projectLoadState === 'loading' || agentSelection.status === 'loading' || copying}
      >
        <DialogHeader className="border-b px-6 pt-6 pb-4">
          <DialogTitle>{t('skills.copyToProject.title', { name: skill?.name })}</DialogTitle>
          <DialogDescription className="sr-only">
            {t('skills.copyToProject.ariaDescription')}
          </DialogDescription>
        </DialogHeader>

        <div
          data-testid="copy-to-project-dialog-body"
          className="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden"
        >
          <div className={hasWorkflowFeedback ? 'space-y-3 px-6 pt-4' : ''}>
            {copyOutcome?.status === 'partial' ? (
              <Alert role="alert" variant="destructive">
                <AlertDescription>
                  {t('skills.copyToProject.partialError', {
                    success: copyOutcome.succeededProjectIds.length,
                    fail: copyOutcome.failedProjectIds.length,
                  })}
                </AlertDescription>
              </Alert>
            ) : copyOutcome?.status === 'recoveryRequired' ? (
              <Alert role="alert" variant="destructive">
                <AlertDescription>
                  <p>{t('skills.copyToProject.recoveryDescription')}</p>
                  {copyOutcome.recovery.map((action) => (
                    <RecoveryActions key={action.resourceId} recovery={action} />
                  ))}
                </AlertDescription>
              </Alert>
            ) : copyOutcome?.status === 'failed' ? (
              <Alert role="alert" variant="destructive">
                <AlertDescription>
                  {copyOutcome.unit?.error
                    ? formatMutationError(copyOutcome.unit.error, t)
                    : t('skills.copyToProject.copyError')}
                </AlertDescription>
              </Alert>
            ) : null}
            {copyOutcome?.status === 'partial' && copyOutcome.recovery?.length ? (
              <div className="space-y-2" role="status">
                <p className="text-sm text-destructive">
                  {t('skills.copyToProject.recoveryDescription')}
                </p>
                {copyOutcome.recovery.map((action) => (
                  <RecoveryActions key={action.resourceId} recovery={action} />
                ))}
              </div>
            ) : null}
          </div>

          <div className="grid min-h-0 min-w-0 grid-rows-[minmax(10rem,0.85fr)_minmax(12rem,1.15fr)] gap-4 px-6 py-4 md:grid-cols-[minmax(16rem,0.8fr)_minmax(24rem,1.2fr)] md:grid-rows-1 md:gap-8">
            <section
              className="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] border-b pb-4 md:border-r md:border-b-0 md:pb-0 md:pr-8"
              aria-labelledby="copy-target-projects-title"
            >
              <div className="space-y-4 pb-3">
                <div className="flex items-center justify-between gap-3">
                  <h2 id="copy-target-projects-title" className="text-sm font-semibold">
                    {t('skills.copyToProject.targetProjects')}
                  </h2>
                  {selected.size > 0 ? (
                    <span className="text-xs text-muted-foreground">
                      {t('skills.copyToProject.selectedProjects', { count: selected.size })}
                    </span>
                  ) : null}
                </div>

                {environments.length > 1 || targetEnvironmentSelection.kind === 'missing' ? (
                  <div className="space-y-1.5">
                    <Label>{t('skills.copyToProject.targetEnvironment')}</Label>
                    <Select
                      value={targetEnvironmentSelection.kind === 'valid'
                        ? targetEnvironmentSelection.key
                        : ''}
                      onValueChange={(value) => {
                        if (!environments.some(
                          (entry) => environmentKey(entry.environment) === value,
                        )) return;
                        setTargetEnvironmentSelection({ kind: 'valid', key: value });
                        clearTargetProjectState();
                        setProjectLoadState('loading');
                      }}
                    >
                      <SelectTrigger aria-label={t('skills.copyToProject.targetEnvironment')}>
                        <SelectValue
                          placeholder={t('skills.copyToProject.targetEnvironmentMissing')}
                        />
                      </SelectTrigger>
                      <SelectContent>
                        {environments.map((environment) => (
                          <SelectItem
                            key={environmentKey(environment.environment)}
                            value={environmentKey(environment.environment)}
                          >
                            {environment.displayName}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                ) : null}
              </div>

              <div
                data-testid="copy-target-projects-scroll"
                className="min-h-0 space-y-1.5 overflow-y-auto overscroll-contain pr-1 [scrollbar-gutter:stable]"
              >
                {!targetEnvironment ? (
                  <Alert>
                    <AlertDescription>
                      {t('skills.copyToProject.targetEnvironmentMissing')}
                    </AlertDescription>
                  </Alert>
                ) : projectLoadState === 'loading' || projectLoadState === 'idle' ? (
                  <div role="status" aria-live="polite" className="space-y-2">
                    <span className="sr-only">{t('common.loading')}</span>
                    <Skeleton className="h-12 w-full" />
                    <Skeleton className="h-12 w-full" />
                    <Skeleton className="h-12 w-4/5" />
                  </div>
                ) : projectLoadState === 'error' ? (
                  <Alert>
                    <AlertDescription>
                      <p>{t(projectLoadFailure === 'environment'
                        ? 'skills.copyToProject.targetEnvironmentConnectionError'
                        : 'skills.copyToProject.projectsLoadError')}</p>
                      <Button
                        variant="link"
                        size="sm"
                        className="h-auto p-0"
                        onClick={() => setProjectLoadAttempt((attempt) => attempt + 1)}
                      >
                        {t('common.retry')}
                      </Button>
                    </AlertDescription>
                  </Alert>
                ) : availableProjects.length > 0 ? (
                  <>
                    {presenceState === 'error' ? (
                      <div
                        role="status"
                        aria-label={t('skills.copyToProject.presenceUnknown')}
                        className="mb-2"
                      >
                        <Alert>
                          <AlertDescription>
                            {t('skills.copyToProject.presenceUnknown')}
                          </AlertDescription>
                        </Alert>
                      </div>
                    ) : null}
                    {availableProjects.map((project) => {
                      const projectId = project.binding.id;
                      const presence = presenceByProject.get(projectId);
                      const failedUnit = failedUnitByProjectId.get(projectId);
                      return (
                        <label
                          key={projectId}
                          className="flex cursor-pointer items-start gap-3 rounded-md p-2 hover:bg-muted/50"
                        >
                          <Checkbox
                            className="mt-0.5"
                            checked={selected.has(projectId)}
                            onCheckedChange={() => toggleProject(projectId)}
                          />
                          <Folder className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                          <span className="min-w-0 flex-1">
                            <ProjectIdentity
                              project={project}
                              nameClassName="text-sm"
                              pathClassName="text-xs text-muted-foreground"
                            />
                            {failedUnit ? (
                              <span className="mt-0.5 block text-xs text-destructive">
                                {failedUnit.error
                                  ? formatMutationError(failedUnit.error, t)
                                  : t('skills.copyToProject.copyError')}
                              </span>
                            ) : null}
                          </span>
                          {presence === 'installed' ? (
                            <Badge variant="outline" className="shrink-0 text-xs text-warning">
                              {t('skills.copyToProject.installed')}
                            </Badge>
                          ) : presenceState === 'error' && presence === 'unknown' ? (
                            <Badge
                              variant="outline"
                              className="shrink-0 text-xs text-muted-foreground"
                            >
                              {t('skills.copyToProject.unknown')}
                            </Badge>
                          ) : null}
                        </label>
                      );
                    })}
                    {selectedExistingCount > 0 ? (
                      <div className="mt-2 flex items-start gap-1.5 rounded-md bg-warning/10 px-2.5 py-2">
                        <AlertTriangle className="mt-px h-3.5 w-3.5 shrink-0 text-warning" />
                        <p className="text-xs leading-relaxed text-warning">
                          {t('skills.copyToProject.overwriteWarning', {
                            count: selectedExistingCount,
                          })}
                        </p>
                      </div>
                    ) : null}
                  </>
                ) : (
                  <p className="py-4 text-center text-sm text-muted-foreground">
                    {t('skills.copyToProject.noProjects')}
                  </p>
                )}
              </div>
            </section>

            <section
              className="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)]"
              aria-labelledby="copy-agent-settings-title"
            >
              <h2 id="copy-agent-settings-title" className="pb-3 text-sm font-semibold">
                {t('agentSelection.copyTitle')}
              </h2>
              <div
                data-testid="copy-agent-settings-scroll"
                className="min-h-0 space-y-6 overflow-y-auto overscroll-contain pr-1 [scrollbar-gutter:stable]"
              >
                <AgentSelectionPanel
                  usage="copyToProject"
                  controller={agentSelection}
                  disabled={copying}
                  emptyMessage={t('agentSelection.installEmpty')}
                  modeClassName="flex-col items-start gap-2"
                />
              </div>
            </section>
          </div>
        </div>

        <DialogFooter className="border-t px-6 py-4">
          <Button variant="outline" onClick={onClose} disabled={copying}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={handleCopy}
            disabled={
              writeBlocked
              || copying
              || !targetEnvironment
              || projectLoadState !== 'ready'
              || selected.size === 0
              || agentSelection.status !== 'ready'
              || (agentSelection.status === 'ready' && agentSelection.requiresReconfirmation)
            }
          >
            {copying ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t('common.loading')}
              </>
            ) : (
              copyOutcome?.status === 'partial' && copyOutcome.retryableProjectIds.length > 0
                ? t('skills.copyToProject.retryFailed')
                : t('skills.copyToProject.copy', { count: selected.size })
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
