import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2, RotateCcw } from 'lucide-react';
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
import { useSkillsDataStore } from '@/stores/skills-data';
import type { UpdatePlan } from '@/stores/skills-utils';
import type { AgentType } from '@/bindings';

interface UpdatePlanDialogProps {
  open: boolean;
  plan: UpdatePlan | null;
  agentDisplayNames?: Map<AgentType, string>;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => Promise<void>;
  onRetryFailed?: () => Promise<void>;
}

function AgentList({
  agents,
  agentDisplayNames,
}: {
  agents: AgentType[];
  agentDisplayNames?: Map<AgentType, string>;
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

export function UpdatePlanDialog({
  open,
  plan,
  agentDisplayNames,
  onOpenChange,
  onConfirm,
  onRetryFailed,
}: UpdatePlanDialogProps) {
  const { t } = useTranslation();
  const [running, setRunning] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [completed, setCompleted] = useState(false);
  const lastUpdateResults = useSkillsDataStore((s) => s.lastUpdateResults);
  const lastFailedUpdateNames = useSkillsDataStore((s) => s.lastFailedUpdateNames);

  useEffect(() => {
    if (open) {
      setRunning(false);
      setRetrying(false);
      setCompleted(false);
    }
  }, [open, plan]);

  const resultCounts = useMemo(() => {
    const results = lastUpdateResults ?? [];
    return {
      success: results.filter((item) => item.status === 'success').length,
      partial: results.filter((item) => item.status === 'partial').length,
      failed: results.filter((item) => item.status === 'failed').length,
      skipped: results.filter((item) => item.status === 'skipped').length,
    };
  }, [lastUpdateResults]);

  if (!plan) return null;

  const handleConfirm = async () => {
    setRunning(true);
    try {
      await onConfirm();
      setCompleted(true);
    } finally {
      setRunning(false);
    }
  };

  const handleRetryFailed = async () => {
    if (!onRetryFailed) return;
    setRetrying(true);
    try {
      await onRetryFailed();
    } finally {
      setRetrying(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl gap-0">
        <DialogHeader className="pb-4 border-b border-border">
          <DialogTitle>{t('skills.updatePlan.readyTitle', { count: plan.updatableCount })}</DialogTitle>
          <DialogDescription>{t('skills.updatePlan.readyDescription')}</DialogDescription>
        </DialogHeader>

        <div className="py-4 space-y-4 max-h-[60vh] overflow-y-auto">
          {!completed ? (
            <>
              {plan.groups.map((group) => (
                <div key={group.id} className="overflow-hidden rounded-md border border-border/70 bg-background/60">
                  <div className="flex items-center justify-between gap-3 border-b border-border/60 px-3 py-2.5">
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium">{group.source}</p>
                      {group.gitRef ? (
                        <p className="mt-0.5 text-xs text-muted-foreground">
                          {t('skills.refBadge', { ref: group.gitRef })}
                        </p>
                      ) : null}
                    </div>
                    <span className="shrink-0 rounded-full bg-muted px-2 py-1 text-xs font-medium text-muted-foreground">
                      {t('skills.updatePlan.skillCount', { count: group.skillNames.length })}
                    </span>
                  </div>
                  <div className="divide-y divide-border/60">
                    {group.skillRows.map((row) => (
                      <div
                        key={row.name}
                        className="grid gap-1.5 px-3 py-2.5 sm:grid-cols-[minmax(0,1fr)_minmax(12rem,1.15fr)] sm:items-center"
                      >
                        <span className="min-w-0 truncate text-sm font-medium">{row.name}</span>
                        <AgentList agents={row.agents} agentDisplayNames={agentDisplayNames} />
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </>
          ) : null}

          {completed ? (
            <div className="rounded-md border border-border p-3">
              <div className="flex items-center gap-2 text-sm font-medium">
                <CheckCircle2 className="h-4 w-4 text-success" />
                {t('skills.updatePlan.resultTitle')}
              </div>
              <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
                <span>{t('skills.updatePlan.resultSuccess', { count: resultCounts.success })}</span>
                <span>{t('skills.updatePlan.resultPartial', { count: resultCounts.partial })}</span>
                <span>{t('skills.updatePlan.resultFailed', { count: resultCounts.failed })}</span>
                <span>{t('skills.updatePlan.resultSkipped', { count: resultCounts.skipped })}</span>
              </div>
              {lastUpdateResults?.length ? (
                <div className="mt-3 space-y-2">
                  {lastUpdateResults.map((item) => (
                    <div key={item.name} className="rounded-md border border-border/70 p-2">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="text-sm font-medium">{item.name}</span>
                        <Badge variant="outline" className="text-xs">
                          {item.status}
                        </Badge>
                        {item.reason ? (
                          <span className="text-xs text-muted-foreground">{item.reason}</span>
                        ) : null}
                      </div>
                      {item.error ? (
                        <p className="mt-1 text-xs text-destructive">{item.error}</p>
                      ) : null}
                      {item.agentResults?.length ? (
                        <div className="mt-2 space-y-1">
                          {item.agentResults.map((agentResult) => (
                            <div
                              key={`${item.name}:${agentResult.agent}`}
                              className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground"
                            >
                              <span className="font-medium text-foreground">{agentResult.agent}</span>
                              <Badge variant="secondary" className="text-[11px]">
                                {agentResult.status}
                              </Badge>
                              {agentResult.mode ? (
                                <Badge variant="outline" className="text-[11px]">
                                  {agentResult.mode}
                                </Badge>
                              ) : null}
                              {agentResult.error ? (
                                <span className="text-destructive">{agentResult.error}</span>
                              ) : null}
                            </div>
                          ))}
                        </div>
                      ) : null}
                    </div>
                  ))}
                </div>
              ) : null}
            </div>
          ) : null}
        </div>

        <DialogFooter className="pt-4 border-t border-border">
          {completed ? (
            <>
              {onRetryFailed && lastFailedUpdateNames.length > 0 ? (
                <Button
                  variant="outline"
                  title={t('skills.updatePlan.retryFailed')}
                  disabled={retrying}
                  onClick={() => {
                    void handleRetryFailed();
                  }}
                >
                  {retrying ? <RotateCcw className="h-4 w-4 animate-spin" /> : null}
                  {t('skills.updatePlan.retryFailed')}
                </Button>
              ) : null}
              <Button onClick={() => onOpenChange(false)}>{t('common.close')}</Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)} disabled={running}>
                {t('common.cancel')}
              </Button>
              <Button onClick={handleConfirm} disabled={running || plan.updatableCount === 0}>
                {running ? <RotateCcw className="h-4 w-4 animate-spin" /> : null}
                {t('skills.updatePlan.confirm')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
