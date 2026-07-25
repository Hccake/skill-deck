// src/components/skills/CopyToProjectDialog.tsx
import { useState, useCallback, useMemo, useEffect, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Folder, Info, Loader2 } from 'lucide-react';
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
import { environmentKey, sameEnvironment } from '@/lib/context';
import type { ContextRef, EnvironmentInfo, EnvironmentRef, InstalledSkill, ProjectInfo } from '@/bindings';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { useMutationStore } from '@/stores/mutation';
import type { CopyOutcome } from '@/workflows/skill-copy';

export interface CopyTargetSelection {
  environment: EnvironmentRef;
  projectIds: string[];
}

interface CopyToProjectDialogProps {
  skill: InstalledSkill | null;
  sourceContext: ContextRef;
  environments: EnvironmentInfo[];
  projectsByEnvironment: Record<string, ProjectInfo[]>;
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

const SOURCE_INFO_LIMIT_REASONS = new Set(['missing-skill-path', 'missingRemoteHash']);
type ProjectLoadState = 'idle' | 'loading' | 'ready' | 'error';
type PresenceState = 'idle' | 'loading' | 'ready' | 'error';
type ProjectPresence = 'installed' | 'absent' | 'unknown';

export const CopyToProjectDialog = memo(function CopyToProjectDialog({
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
      skill={skill}
      sourceContext={sourceContext}
      {...sessionProps}
    />
  );
});

function CopyToProjectDialogSession({
  skill,
  sourceContext,
  environments,
  projectsByEnvironment,
  onLoadProjects,
  checkExistence,
  onClose,
  onCopy,
}: CopyToProjectDialogProps) {
  const { t } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [copying, setCopying] = useState(false);
  const [projectLoadState, setProjectLoadState] = useState<ProjectLoadState>('idle');
  const [projectLoadAttempt, setProjectLoadAttempt] = useState(0);
  const [presenceState, setPresenceState] = useState<PresenceState>('idle');
  const [targetEnvironmentKey, setTargetEnvironmentKey] = useState(
    environmentKey(sourceContext.environment),
  );
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [presenceByProject, setPresenceByProject] = useState<Map<string, ProjectPresence>>(
    () => new Map(),
  );
  const [completedProjectIds, setCompletedProjectIds] = useState<Set<string>>(new Set());
  const [copyOutcome, setCopyOutcome] = useState<CopyOutcome | null>(null);

  const targetEnvironment = environments.find(
    (entry) => environmentKey(entry.environment) === targetEnvironmentKey,
  )?.environment ?? sourceContext.environment;

  const availableProjects = useMemo(
    () => (projectsByEnvironment[targetEnvironmentKey] ?? []).filter((project) => !completedProjectIds.has(project.binding.id) && !(
      sameEnvironment(targetEnvironment, sourceContext.environment)
      && sourceContext.scope.scope === 'project'
      && project.binding.id === sourceContext.scope.project_id
    )),
    [completedProjectIds, projectsByEnvironment, sourceContext, targetEnvironment, targetEnvironmentKey],
  );

  useEffect(() => {
    if (!skill) return;
    let cancelled = false;
    setProjectLoadState('loading');
    setPresenceState('idle');
    setPresenceByProject(new Map());
    void onLoadProjects(targetEnvironment)
      .then(() => {
        if (!cancelled) setProjectLoadState('ready');
      })
      .catch(() => {
        if (!cancelled) setProjectLoadState('error');
      });
    return () => { cancelled = true; };
  }, [onLoadProjects, projectLoadAttempt, skill, targetEnvironment]);

  useEffect(() => {
    if (!skill || projectLoadState !== 'ready') return;
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

  const showSourceInfoNote = useMemo(() => {
    if (!skill) return false;
    if (!skill.source && !skill.sourceUrl) return true;
    return SOURCE_INFO_LIMIT_REASONS.has(skill.updateReason ?? '');
  }, [skill]);

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
    setCopying(true);
    try {
      const outcome = await onCopy({ environment: targetEnvironment, projectIds: Array.from(selected) });
      if (!outcome || outcome.status === 'succeeded') {
        onClose();
        return;
      }
      setCopyOutcome(outcome);
      if (outcome.status === 'partial') {
        setCompletedProjectIds((previous) => new Set([
          ...previous,
          ...outcome.succeededProjectIds,
        ]));
        setSelected(new Set(outcome.retryableProjectIds));
      } else if (outcome.status === 'recoveryRequired') {
        setSelected(new Set());
      }
    } finally {
      setCopying(false);
    }
  }, [onClose, onCopy, selected, targetEnvironment]);

  return (
    <Dialog open={!!skill} onOpenChange={(open) => !open && !copying && onClose()}>
      <DialogContent
        className="h-[min(32rem,calc(100dvh-2rem))] grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-md"
        dismissible={!copying}
        closeLabel={t('common.close')}
        aria-busy={projectLoadState === 'loading' || copying}
      >
        <DialogHeader className="border-b px-6 pt-6 pb-4">
          <DialogTitle>{t('skills.copyToProject.title')}</DialogTitle>
          <DialogDescription>
            {t('skills.copyToProject.description', { name: skill?.name })}
          </DialogDescription>
        </DialogHeader>

        <div
          data-testid="copy-to-project-dialog-body"
          className="min-h-0 space-y-4 overflow-y-auto overscroll-contain px-6 py-4"
        >
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
            <AlertDescription>{t('skills.copyToProject.copyError')}</AlertDescription>
          </Alert>
        ) : null}
        {copyOutcome?.status === 'partial' && copyOutcome.recovery?.length ? (
          <div className="space-y-2" role="status">
            <p className="text-sm text-destructive">{t('skills.copyToProject.recoveryDescription')}</p>
            {copyOutcome.recovery.map((action) => (
              <RecoveryActions key={action.resourceId} recovery={action} />
            ))}
          </div>
        ) : null}
        {showSourceInfoNote ? (
          <div role="note" className="flex items-start gap-1.5 rounded-md bg-muted/40 px-2.5 py-2">
            <Info className="h-3.5 w-3.5 shrink-0 mt-px text-muted-foreground" />
            <p className="text-xs text-muted-foreground leading-relaxed">
              {t('skills.copyToProject.metadataWarning')}
            </p>
          </div>
        ) : null}

        <div className="space-y-1.5">
          <Label>{t('skills.copyToProject.targetEnvironment')}</Label>
          <Select
            value={targetEnvironmentKey}
            onValueChange={(value) => {
              setTargetEnvironmentKey(value);
              setSelected(new Set());
              setCompletedProjectIds(new Set());
              setCopyOutcome(null);
              setProjectLoadState('loading');
              setPresenceState('idle');
              setPresenceByProject(new Map());
            }}
          >
            <SelectTrigger aria-label={t('skills.copyToProject.targetEnvironment')}>
              <SelectValue />
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

        <div className="space-y-1.5">
          {projectLoadState === 'loading' || projectLoadState === 'idle' ? (
            <div role="status" aria-live="polite" className="space-y-2">
              <span className="sr-only">{t('common.loading')}</span>
              <Skeleton className="h-12 w-full" />
              <Skeleton className="h-12 w-full" />
              <Skeleton className="h-12 w-4/5" />
            </div>
          ) : projectLoadState === 'error' ? (
            <Alert>
              <AlertDescription>
                <p>{t('skills.copyToProject.projectsLoadError')}</p>
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
                return (
                  <label
                    key={projectId}
                    className="flex items-center gap-3 p-2 rounded-md hover:bg-muted/50 cursor-pointer"
                  >
                    <Checkbox
                      checked={selected.has(projectId)}
                      onCheckedChange={() => toggleProject(projectId)}
                    />
                    <Folder className="h-4 w-4 text-muted-foreground shrink-0" />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm">
                        {project.binding.displayName || project.binding.nativePath}
                      </span>
                      {project.binding.displayName ? (
                        <span className="block truncate text-xs text-muted-foreground">
                          {project.binding.nativePath}
                        </span>
                      ) : null}
                    </span>
                    {presence === 'installed' ? (
                      <Badge variant="outline" className="text-xs text-warning shrink-0">
                        {t('skills.copyToProject.installed')}
                      </Badge>
                    ) : presenceState === 'error' && presence === 'unknown' ? (
                      <Badge variant="outline" className="text-xs text-muted-foreground shrink-0">
                        {t('skills.copyToProject.unknown')}
                      </Badge>
                    ) : null}
                  </label>
                );
              })}
              {selectedExistingCount > 0 ? (
                <div className="flex items-start gap-1.5 rounded-md bg-warning/10 px-2.5 py-2 mt-2">
                  <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-px text-warning" />
                  <p className="text-xs text-warning leading-relaxed">
                    {t('skills.copyToProject.overwriteWarning', { count: selectedExistingCount })}
                  </p>
                </div>
              ) : null}
            </>
          ) : (
            <p className="text-sm text-muted-foreground py-4 text-center">
              {t('skills.copyToProject.noProjects')}
            </p>
          )}
        </div>
        </div>

        <DialogFooter className="border-t px-6 py-4">
          <Button variant="outline" onClick={onClose} disabled={copying}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={handleCopy}
            disabled={writeBlocked || copying || projectLoadState !== 'ready' || selected.size === 0}
          >
            {copying ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t('common.loading')}
              </>
            ) : (
              copyOutcome?.status === 'partial'
                ? t('skills.copyToProject.retryFailed')
                : t('skills.copyToProject.copy', { count: selected.size })
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
