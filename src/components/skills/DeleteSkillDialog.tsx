// src/components/skills/DeleteSkillDialog.tsx
import { useState, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { useSkillsStore } from '@/stores/skills';
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
import { Globe, Folder, Link, Loader2, AlertTriangle } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { AgentType, SkillAgentDetails } from '@/bindings';

/** 从 agentDetails 计算默认全选的 Set（rerender-derived-state-no-effect） */
function buildDefaultSelection(details: SkillAgentDetails | null): Set<AgentType> {
  if (!details?.independentAgents?.length) return new Set();
  return new Set(details.independentAgents.map((a) => a.agent));
}

export const DeleteSkillDialog = memo(function DeleteSkillDialog() {
  const { t } = useTranslation();
  const target = useSkillsStore((s) => s.deleteTarget);
  const agentDetails = useSkillsStore((s) => s.agentDetails);
  const loadingDetails = useSkillsStore((s) => s.loadingAgentDetails);
  const closeDelete = useSkillsStore((s) => s.closeDelete);
  const deleteSkillAction = useSkillsStore((s) => s.deleteSkill);

  const [isDeleting, setIsDeleting] = useState(false);
  const [deleteCanonical, setDeleteCanonical] = useState(false);
  const [selectedAgents, setSelectedAgents] = useState<Set<AgentType>>(new Set());

  // render-time reset: agentDetails 变化时重置状态（替代 useEffect）
  const [prevAgentDetails, setPrevAgentDetails] = useState(agentDetails);
  if (agentDetails !== prevAgentDetails) {
    setPrevAgentDetails(agentDetails);
    setSelectedAgents(buildDefaultSelection(agentDetails));
    setDeleteCanonical(false);
  }

  // 直接计算的 derived state
  const hasUniversal = (agentDetails?.universalAgents?.length ?? 0) > 0;
  const hasIndependent = (agentDetails?.independentAgents?.length ?? 0) > 0;
  const hasAnyAgent = hasUniversal || hasIndependent;
  const selectedCount = selectedAgents.size;
  const canConfirm = hasAnyAgent ? (deleteCanonical || selectedCount > 0) : true;

  // 级联切换：勾选共享目录 → 自动全选并锁定独立 agent
  const handleToggleCanonical = useCallback((checked: boolean) => {
    setDeleteCanonical(checked);
    if (checked && agentDetails) {
      setSelectedAgents(buildDefaultSelection(agentDetails));
    }
  }, [agentDetails]);

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

  const handleConfirm = useCallback(async () => {
    setIsDeleting(true);
    try {
      if (deleteCanonical || !hasAnyAgent) {
        await deleteSkillAction({ fullRemoval: true });
      } else {
        await deleteSkillAction({
          fullRemoval: false,
          agents: Array.from(selectedAgents),
        });
      }
    } finally {
      setIsDeleting(false);
    }
  }, [deleteCanonical, hasAnyAgent, selectedAgents, deleteSkillAction]);

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
            {hasUniversal && (
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
                  {agentDetails.universalAgents.map(([agentId, name]) => (
                    <Badge key={agentId} variant="secondary" className="text-xs">
                      {name}
                    </Badge>
                  ))}
                </div>

                {deleteCanonical && (
                  <div className="flex items-start gap-1.5 rounded-md bg-warning/10 px-2.5 py-2">
                    <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-px text-warning" />
                    <p className="text-xs text-warning leading-relaxed">
                      {t('skills.deleteConfirm.canonicalWarning')}
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
                          checked={deleteCanonical || selectedAgents.has(info.agent)}
                          disabled={deleteCanonical}
                          onCheckedChange={() => toggleAgent(info.agent)}
                        />
                        <Label
                          htmlFor={`agent-${info.agent}`}
                          className={cn(
                            'text-sm',
                            deleteCanonical ? 'text-muted-foreground' : 'cursor-pointer'
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
            disabled={isDeleting || !canConfirm}
          >
            {isDeleting ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t('common.loading')}
              </>
            ) : isFullRemoval ? (
              t('skills.deleteConfirm.confirm')
            ) : (
              t('skills.deleteConfirm.confirmPartial', { count: selectedCount })
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
