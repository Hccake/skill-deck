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
import { environmentKey, sameEnvironment } from '@/lib/context';
import type { ContextRef, EnvironmentInfo, EnvironmentRef, InstalledSkill, ProjectInfo } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

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
  onCopy: (selection: CopyTargetSelection) => Promise<void>;
}

const SOURCE_INFO_LIMIT_REASONS = new Set(['missing-skill-path', 'missingRemoteHash']);
type ProjectLoadState = 'idle' | 'loading' | 'ready' | 'error';
type PresenceState = 'idle' | 'loading' | 'ready' | 'error';
type ProjectPresence = 'installed' | 'absent' | 'unknown';

export const CopyToProjectDialog = memo(function CopyToProjectDialog({
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

  // render-time reset
  const [prevSkill, setPrevSkill] = useState(skill);
  if (skill !== prevSkill) {
    setPrevSkill(skill);
    setSelected(new Set());
    setPresenceByProject(new Map());
    setPresenceState('idle');
    setProjectLoadState('idle');
    setTargetEnvironmentKey(environmentKey(sourceContext.environment));
  }

  const targetEnvironment = environments.find(
    (entry) => environmentKey(entry.environment) === targetEnvironmentKey,
  )?.environment ?? sourceContext.environment;

  const availableProjects = useMemo(
    () => (projectsByEnvironment[targetEnvironmentKey] ?? []).filter((project) => !(
      sameEnvironment(targetEnvironment, sourceContext.environment)
      && sourceContext.scope.scope === 'project'
      && project.binding.id === sourceContext.scope.project_id
    )),
    [projectsByEnvironment, sourceContext, targetEnvironment, targetEnvironmentKey],
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
      await onCopy({ environment: targetEnvironment, projectIds: Array.from(selected) });
    } finally {
      setCopying(false);
    }
  }, [onCopy, selected, targetEnvironment]);

  return (
    <Dialog open={!!skill} onOpenChange={(open) => !open && !copying && onClose()}>
      <DialogContent className="sm:max-w-md gap-0">
        <DialogHeader>
          <DialogTitle>{t('skills.copyToProject.title')}</DialogTitle>
          <DialogDescription>
            {t('skills.copyToProject.description', { name: skill?.name })}
          </DialogDescription>
        </DialogHeader>

        {showSourceInfoNote ? (
          <div role="note" className="mt-4 flex items-start gap-1.5 rounded-md bg-muted/40 px-2.5 py-2">
            <Info className="h-3.5 w-3.5 shrink-0 mt-px text-muted-foreground" />
            <p className="text-xs text-muted-foreground leading-relaxed">
              {t('skills.copyToProject.metadataWarning')}
            </p>
          </div>
        ) : null}

        <div className="mt-4 space-y-1.5">
          <Label>{t('skills.copyToProject.targetEnvironment')}</Label>
          <Select
            value={targetEnvironmentKey}
            onValueChange={(value) => {
              setTargetEnvironmentKey(value);
              setSelected(new Set());
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

        <div className={showSourceInfoNote ? 'mt-2 space-y-1.5 max-h-[50vh] overflow-y-auto' : 'mt-4 space-y-1.5 max-h-[50vh] overflow-y-auto'}>
          {projectLoadState === 'loading' || projectLoadState === 'idle' ? (
            <p role="status" aria-live="polite" className="py-4 text-center text-sm text-muted-foreground">
              {t('common.loading')}
            </p>
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

        <DialogFooter className="mt-4">
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
              t('skills.copyToProject.copy', { count: selected.size })
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
