// src/components/skills/DeleteSkillDialog.tsx
import { useState, useCallback, memo, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useSkillDialogStore } from '@/stores/skill-dialog';
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
import { Skeleton } from '@/components/ui/skeleton';
import { Globe, Folder, Link, Loader2, AlertTriangle, Bot } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { AgentType, InstallTargetInfo, InstallTargetSpec, SkillAgentDetails } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

/** 从 agentDetails 计算默认全选的 Set（rerender-derived-state-no-effect） */
function buildDefaultSelection(details: SkillAgentDetails | null): Set<AgentType> {
  if (!details?.independentAgents?.length) return new Set();
  return new Set(details.independentAgents.map((a) => a.agent));
}

function targetKey(target: Pick<InstallTargetInfo, 'agent' | 'subagent'> | InstallTargetSpec) {
  return `${target.agent}:${target.subagent ?? 'root'}`;
}

function targetSpec(target: Pick<InstallTargetInfo, 'agent' | 'subagent'>): InstallTargetSpec {
  return {
    agent: target.agent,
    subagent: target.subagent ?? null,
  };
}

export const DeleteSkillDialog = memo(function DeleteSkillDialog() {
  const { t } = useTranslation();
  const target = useSkillDialogStore((s) => s.deleteTarget);
  const agentDetails = useSkillDialogStore((s) => s.agentDetails);
  const loadingDetails = useSkillDialogStore((s) => s.loadingAgentDetails);
  const closeDelete = useSkillDialogStore((s) => s.closeDelete);
  const deleteSkillAction = useSkillDialogStore((s) => s.deleteSkill);
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);

  const [isDeleting, setIsDeleting] = useState(false);
  const [deleteCanonical, setDeleteCanonical] = useState(false);
  const [selectedAgents, setSelectedAgents] = useState<Set<AgentType>>(new Set());
  const [selectedEveTargets, setSelectedEveTargets] = useState<Set<string>>(
    () => new Set((agentDetails?.eveTargets ?? []).map(targetKey))
  );

  // render-time reset: agentDetails 变化时重置状态（替代 useEffect）
  const [prevAgentDetails, setPrevAgentDetails] = useState(agentDetails);
  if (agentDetails !== prevAgentDetails) {
    setPrevAgentDetails(agentDetails);
    setSelectedAgents(buildDefaultSelection(agentDetails));
    setSelectedEveTargets(new Set((agentDetails?.eveTargets ?? []).map(targetKey)));
    setDeleteCanonical(false);
  }

  // 直接计算的 derived state
  const hasAutomatic = (agentDetails?.automaticAgents?.length ?? 0) > 0;
  const hasIndependent = (agentDetails?.independentAgents?.length ?? 0) > 0;
  const eveTargets = useMemo(() => agentDetails?.eveTargets ?? [], [agentDetails?.eveTargets]);
  const hasEveTargets = eveTargets.length > 0;
  const hasAnyAgent = hasAutomatic || hasIndependent || hasEveTargets;
  const selectedCount = selectedAgents.size;
  const selectedEveTargetCount = selectedEveTargets.size;
  const selectedRemovalCount = selectedCount + selectedEveTargetCount;
  const canConfirm = hasAnyAgent
    ? (deleteCanonical || selectedRemovalCount > 0)
    : true;

  const retainedIndependentCount = deleteCanonical && agentDetails
    ? agentDetails.independentAgents.filter((info) => !selectedAgents.has(info.agent)).length
    : 0;

  const handleToggleCanonical = useCallback((checked: boolean) => {
    setDeleteCanonical(checked);
  }, []);

  // rerender-functional-setstate：空 deps，stable callback
  const toggleAgent = useCallback((agent: AgentType) => {
    setSelectedAgents((prev) => {
      const next = new Set(prev);
      if (next.has(agent)) {
        next.delete(agent);
      } else {
        next.add(agent);
      }
      return next;
    });
  }, []);

  const toggleEveTarget = useCallback((target: InstallTargetInfo) => {
    const key = targetKey(target);
    setSelectedEveTargets((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }, []);

  const handleConfirm = useCallback(async () => {
    setIsDeleting(true);
    try {
      const selectedAgentList = Array.from(selectedAgents);
      const selectedTargetSpecs = eveTargets
        .filter((target) => selectedEveTargets.has(targetKey(target)))
        .map(targetSpec);
      const fullRemovalEveTargets = deleteCanonical && hasEveTargets
        ? selectedTargetSpecs
        : undefined;

      if (deleteCanonical || !hasAnyAgent) {
        await deleteSkillAction({
          fullRemoval: true,
          agents: deleteCanonical ? selectedAgentList : undefined,
          agentTargets: fullRemovalEveTargets,
        });
      } else {
        await deleteSkillAction({
          fullRemoval: false,
          agents: selectedAgentList,
          agentTargets: selectedTargetSpecs.length > 0 ? selectedTargetSpecs : undefined,
        });
      }
    } finally {
      setIsDeleting(false);
    }
  }, [
    deleteCanonical,
    hasAnyAgent,
    hasEveTargets,
    selectedAgents,
    eveTargets,
    selectedEveTargets,
    deleteSkillAction,
  ]);

  const ScopeIcon = target?.scope === 'global' ? Globe : Folder;
  const isFullRemoval = deleteCanonical || !hasAnyAgent;

  return (
    <Dialog open={!!target} onOpenChange={(open) => !open && !isDeleting && closeDelete()}>
      <DialogContent className="sm:max-w-md gap-0">
        <DialogHeader>
          <DialogTitle>{t('skills.deleteConfirm.title')}</DialogTitle>
          <DialogDescription>
            {t('skills.deleteConfirm.description')}
          </DialogDescription>
        </DialogHeader>

        {/* Skill Identity Banner */}
        <div className="flex items-center gap-3 rounded-lg border bg-muted/50 p-3 mt-4">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-accent">
            <ScopeIcon className="h-4 w-4 text-accent-foreground" />
          </div>
          <div className="min-w-0">
            <p className="truncate text-sm font-semibold text-foreground">
              {target?.skill.name}
            </p>
            <p className="text-xs text-muted-foreground">
              {t(`skills.scopeIcon.${target?.scope ?? 'global'}`)}
            </p>
          </div>
        </div>

        {/* Agent Selection */}
        {loadingDetails ? (
          <div className="mt-4 space-y-3">
            <Skeleton className="h-[72px] w-full rounded-lg" />
            <Skeleton className="h-5 w-20 rounded" />
            <Skeleton className="h-5 w-full rounded" />
            <Skeleton className="h-5 w-full rounded" />
          </div>
        ) : agentDetails && hasAnyAgent ? (
          <div className="mt-4 space-y-3">
            {/* Shared Directory Section */}
            {hasAutomatic && (
              <div className={cn(
                'rounded-lg border p-3 space-y-2.5 transition-colors',
                deleteCanonical && 'border-destructive/30 bg-destructive/5'
              )}>
                <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
                  {t('skills.deleteConfirm.sharedDirSection')}
                </p>

                <div className="flex items-start gap-2">
                  <Checkbox
                    id="delete-canonical"
                    checked={deleteCanonical}
                    onCheckedChange={(checked) => handleToggleCanonical(!!checked)}
                  />
                  <Label htmlFor="delete-canonical" className="text-sm cursor-pointer leading-snug">
                    {t('skills.deleteConfirm.deleteCanonical')}
                  </Label>
                </div>

                <div className="flex flex-wrap gap-1 pl-6">
                  {agentDetails.automaticAgents.map(([agentId, name]) => (
                    <Badge key={agentId} variant="secondary" className="text-xs">
                      {name}
                    </Badge>
                  ))}
                </div>

                {deleteCanonical && (
                  <div className="flex items-start gap-1.5 rounded-md bg-warning/10 px-2.5 py-2">
                    <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-px text-warning" />
                    <p className="text-xs text-warning leading-relaxed">
                      {retainedIndependentCount > 0
                        ? t('skills.deleteConfirm.canonicalLeavesPrivateCopiesWarning')
                        : t('skills.deleteConfirm.canonicalWarning')}
                    </p>
                  </div>
                )}
              </div>
            )}

            {/* Independent Agents Section */}
            {hasIndependent && (
              <>
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  <span className="shrink-0 font-medium uppercase tracking-wider">
                    {t('skills.deleteConfirm.independentSection')}
                  </span>
                  <div className="h-px flex-1 bg-border" />
                </div>

                <div className="space-y-1.5">
                  {agentDetails.independentAgents.map((info) => (
                    <div key={info.agent} className="flex items-center justify-between py-0.5">
                      <div className="flex items-center gap-2">
                        <Checkbox
                          id={`agent-${info.agent}`}
                          checked={selectedAgents.has(info.agent)}
                          onCheckedChange={() => toggleAgent(info.agent)}
                        />
                        <Label
                          htmlFor={`agent-${info.agent}`}
                          className={cn(
                            'text-sm',
                            'cursor-pointer'
                          )}
                        >
                          {info.displayName}
                        </Label>
                      </div>
                      {info.isSymlink && (
                        <Link className="h-3 w-3 text-muted-foreground/40" />
                      )}
                    </div>
                  ))}
                </div>
              </>
            )}

            {hasEveTargets && (
              <>
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  <span className="shrink-0 font-medium uppercase tracking-wider">
                    {t('skills.deleteConfirm.eveTargetsSection')}
                  </span>
                  <div className="h-px flex-1 bg-border" />
                </div>

                <div className="space-y-1.5">
                  {eveTargets.map((target) => {
                    const id = `eve-target-${target.targetId}`;
                    return (
                      <div key={target.targetId} className="flex items-center justify-between py-0.5">
                        <div className="flex min-w-0 items-center gap-2">
                          <Checkbox
                            id={id}
                            checked={selectedEveTargets.has(targetKey(target))}
                            onCheckedChange={() => toggleEveTarget(target)}
                          />
                          <Label htmlFor={id} className="min-w-0 cursor-pointer text-sm">
                            <span className="flex items-center gap-1.5">
                              <Bot className="h-3.5 w-3.5 text-muted-foreground" />
                              {target.displayName}
                            </span>
                          </Label>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </>
            )}
          </div>
        ) : (
          /* agentDetails 加载失败或无 agent 安装 → 简单确认 */
          <p className="mt-4 text-sm text-muted-foreground">
            {agentDetails
              ? t('skills.deleteConfirm.noAgentsInstalled')
              : t('skills.deleteConfirm.fallbackConfirm')}
          </p>
        )}

        <DialogFooter className="mt-4">
          <Button variant="outline" onClick={closeDelete} disabled={isDeleting}>
            {t('skills.deleteConfirm.cancel')}
          </Button>
          <Button
            variant={isFullRemoval ? 'destructive' : 'default'}
            onClick={handleConfirm}
            disabled={writeBlocked || isDeleting || !canConfirm}
          >
            {isDeleting ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t('common.loading')}
              </>
            ) : isFullRemoval ? (
              t('skills.deleteConfirm.confirm')
            ) : (
              t('skills.deleteConfirm.confirmPartial', { count: selectedRemovalCount })
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
