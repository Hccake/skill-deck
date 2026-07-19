// src/components/skills/SkillDetailPanel.tsx
import { memo, useState, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Link2, Copy, Check, X, RefreshCw, Trash2, ArrowUpCircle, Pencil, FolderOutput, Wrench, AlertTriangle } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { formatTime } from '@/lib/utils';
import type { InstalledSkill, SkillScope, SkillUpdateCheckStatus } from '@/bindings';
import {
  resolveSkillMaintenanceAction,
  isSkillUpdateActive,
  resolveSkillUpdatePhaseI18nKey,
  type SkillUpdateDisplayStatus,
  type SkillUpdateActivePhase,
  resolveUpdateReasonI18nKey,
  resolveUpdateStatusI18nKey,
} from '@/stores/skills-utils';
import { useMutationStore } from '@/stores/mutation';

interface SkillDetailPanelProps {
  skill: InstalledSkill & {
    updateStatus?: SkillUpdateCheckStatus | null;
    updateReason?: string | null;
  };
  content: string | null;
  loading: boolean;
  agentDisplayNames: Map<string, string>;
  updateStatus?: SkillUpdateDisplayStatus;
  isCheckingUpdates?: boolean;
  projectPath?: string;
  onClose: () => void;
  onCheckUpdates?: () => Promise<boolean>;
  onUpdate: (name: string, scope: SkillScope) => void;
  onDelete: (skill: InstalledSkill) => void;
  onRetry: () => void;
  onManageAgents: (skill: InstalledSkill) => void;
  onCopyToProject?: (skill: InstalledSkill) => void;
  onRepairSource?: (skill: InstalledSkill) => void;
}

export const SkillDetailPanel = memo(function SkillDetailPanel({
  skill,
  content,
  loading,
  agentDisplayNames,
  updateStatus,
  isCheckingUpdates = false,
  onClose,
  onCheckUpdates,
  onUpdate,
  onDelete,
  onRetry,
  onManageAgents,
  onCopyToProject,
  onRepairSource,
}: SkillDetailPanelProps) {
  const { t, i18n } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [copied, setCopied] = useState(false);
  const [checkDone, setCheckDone] = useState(false);
  const hideCheckDoneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const defaultAvailableAgents = skill.defaultAvailableAgents ?? [];
  const privateAdaptedAgents = skill.privateAdaptedAgents ?? [];
  const privateCopyAgents = skill.privateCopyAgents ?? [];
  const summaryAgents = [...defaultAvailableAgents, ...privateAdaptedAgents, ...privateCopyAgents];
  const hasAgentSummary = Boolean(
    skill.defaultAvailableAgents || skill.privateAdaptedAgents || skill.privateCopyAgents
  );
  const rawDisplayAgents = skill.cardAgents
    ?? (hasAgentSummary ? summaryAgents : skill.agents);
  const displayAgents = rawDisplayAgents.filter((agentId, index, agents) => agents.indexOf(agentId) === index);
  const duplicateCopyCount = skill.duplicateCopyCount ?? 0;
  const duplicateCopyAgents = skill.duplicateCopyAgents ?? [];
  const duplicateCopyAgentNames = duplicateCopyAgents.map((agentId) => agentDisplayNames.get(agentId) ?? agentId);
  const duplicateCopyAgentSummary = duplicateCopyAgentNames.length > 3
    ? t('skills.detail.extraCopiesAgentSummaryMore', {
      agents: duplicateCopyAgentNames.slice(0, 2).join('、'),
      count: duplicateCopyAgentNames.length,
    })
    : duplicateCopyAgentNames.join('、');
  const duplicateCopyAgentSeparator = i18n.language.startsWith('zh') ? '' : ' ';

  useEffect(() => {
    return () => {
      if (hideCheckDoneTimerRef.current) {
        clearTimeout(hideCheckDoneTimerRef.current);
      }
    };
  }, []);

  const handleCopyPath = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(skill.canonicalPath);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      console.error('Failed to copy path');
    }
  }, [skill.canonicalPath]);

  const handleDelete = useCallback(() => {
    onDelete(skill);
  }, [onDelete, skill]);

  const handleUpdate = useCallback(() => {
    onUpdate(skill.name, skill.scope);
  }, [onUpdate, skill.name, skill.scope]);

  const handleManageAgents = useCallback(() => {
    onManageAgents(skill);
  }, [onManageAgents, skill]);

  const handleCopyToProject = useCallback(() => {
    onCopyToProject?.(skill);
  }, [onCopyToProject, skill]);

  const handleRepairSource = useCallback(() => {
    onRepairSource?.(skill);
  }, [onRepairSource, skill]);

  const handleCheckUpdates = useCallback(async () => {
    if (!onCheckUpdates || isCheckingUpdates) return;

    const succeeded = await onCheckUpdates();
    if (!succeeded) {
      return;
    }

    if (hideCheckDoneTimerRef.current) {
      clearTimeout(hideCheckDoneTimerRef.current);
    }
    setCheckDone(true);
    hideCheckDoneTimerRef.current = setTimeout(() => {
      setCheckDone(false);
      hideCheckDoneTimerRef.current = null;
    }, 800);
  }, [isCheckingUpdates, onCheckUpdates]);

  const activeUpdatePhase = isSkillUpdateActive(updateStatus) ? updateStatus : null;
  const isUpdateInProgress = activeUpdatePhase !== null;
  const showCheckDone = checkDone && !isCheckingUpdates && !skill.hasUpdate;
  const isDeletedUpstream = skill.updateStatus === 'deletedUpstream' || skill.updateReason === 'deletedUpstream';
  const showCannotCheckStatus = isDeletedUpstream
    || skill.updateStatus === 'cannotCheck'
    || skill.canCheckForUpdates === false;
  const canShowUpdateAction = skill.hasUpdate === true && skill.canRunUpdate !== false && !isDeletedUpstream;
  const maintenanceAction = updateStatus ? 'none' : resolveSkillMaintenanceAction(skill);
  const canShowDirectReinstallAction = maintenanceAction === 'direct-reinstall';
  const canShowRepairAction = (maintenanceAction === 'repair-source' || isDeletedUpstream) && Boolean(onRepairSource);
  const repairActionTitle = isDeletedUpstream
    ? t('skills.updatePlan.deletedUpstreamActionRepair')
    : t('skills.actions.repairSource');

  return (
    <div className="h-full flex flex-col overflow-hidden bg-surface">
      {/* 沉浸式滚动文档流 (Scrollable Document Area) */}
      <div className="flex-1 min-h-0 relative">
        <ScrollArea className="absolute inset-0 w-full h-full">
          <div className="px-6 py-6 sm:px-8 sm:py-6 w-full space-y-4">

            {/* Hero title & Abstract */}
            <div className="space-y-3">
              <div className="flex justify-between items-start gap-4">
                <h2 className="min-w-0 flex-1 text-2xl sm:text-3xl font-heading font-extrabold tracking-tight text-foreground leading-tight">
                  {skill.name}
                </h2>
                <div className="flex shrink-0 flex-wrap justify-end gap-1 pt-1">
                  {activeUpdatePhase ? (
                    <UpdatingStatusBadge phase={activeUpdatePhase} />
                  ) : null}
                  {updateStatus === 'done' ? (
                    <Badge variant="outline" className="h-8 px-2 text-xs text-success">
                      {t('skills.updateDone')}
                    </Badge>
                  ) : null}
                  {updateStatus === 'failed' ? (
                    <Badge variant="outline" className="h-8 px-2 text-xs text-destructive">
                      {t('skills.updateFailed')}
                    </Badge>
                  ) : null}
                  {canShowUpdateAction && !updateStatus ? (
                      <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-warning hover:text-warning hover:bg-warning/10 cursor-pointer"
                          title={t('skills.actions.update')}
                          disabled={writeBlocked}
                          onClick={handleUpdate}
                      >
                        <ArrowUpCircle className="h-4 w-4" />
                      </Button>
                  ) : null}
                  {canShowDirectReinstallAction ? (
                    <AlertDialog>
                      <AlertDialogTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-warning hover:text-warning hover:bg-warning/10 cursor-pointer"
                          title={t('skills.actions.reinstall')}
                          disabled={writeBlocked}
                          onClick={(event) => event.stopPropagation()}
                        >
                          <Wrench className="h-4 w-4" />
                        </Button>
                      </AlertDialogTrigger>
                      <AlertDialogContent>
                        <AlertDialogHeader>
                          <AlertDialogTitle>{t('skills.reinstallConfirm.title')}</AlertDialogTitle>
                          <AlertDialogDescription>{t('skills.reinstallConfirm.description')}</AlertDialogDescription>
                        </AlertDialogHeader>
                        <AlertDialogFooter>
                          <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
                          <AlertDialogAction onClick={handleUpdate}>
                            {t('skills.reinstallConfirm.confirm')}
                          </AlertDialogAction>
                        </AlertDialogFooter>
                      </AlertDialogContent>
                    </AlertDialog>
                  ) : null}
                  {canShowRepairAction ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-warning hover:text-warning hover:bg-warning/10 cursor-pointer"
                      title={repairActionTitle}
                      disabled={writeBlocked}
                      onClick={handleRepairSource}
                    >
                      <Wrench className="h-4 w-4" />
                    </Button>
                  ) : null}
                  {!isUpdateInProgress && onCheckUpdates && skill.canCheckForUpdates === true ? (
                    showCheckDone ? (
                      <div
                        className="flex h-8 w-8 items-center justify-center text-success"
                        aria-label={t('skills.updateDone')}
                        title={t('skills.updateDone')}
                      >
                        <Check className="h-4 w-4" />
                      </div>
                    ) : (
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer"
                        title={t('skills.checkUpdates')}
                        disabled={isCheckingUpdates}
                        onClick={() => {
                          void handleCheckUpdates();
                        }}
                      >
                        <RefreshCw className={`h-4 w-4 ${isCheckingUpdates ? 'animate-spin' : ''}`} />
                      </Button>
                    )
                  ) : null}
                  {skill.scope === 'project' && onCopyToProject ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer"
                      title={t('skills.actions.copyToProject')}
                      disabled={writeBlocked}
                      onClick={handleCopyToProject}
                    >
                      <FolderOutput className="h-4 w-4" />
                    </Button>
                  ) : null}
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 text-muted-foreground hover:text-foreground hover:bg-muted/50 cursor-pointer"
                    title={t('skills.manageAgents.title')}
                    disabled={writeBlocked}
                    onClick={handleManageAgents}
                  >
                    <Pencil className="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 text-muted-foreground hover:text-destructive hover:bg-destructive/10 cursor-pointer"
                    title={t('skills.actions.delete')}
                    disabled={writeBlocked}
                    onClick={handleDelete}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 text-muted-foreground hover:text-foreground cursor-pointer"
                    title={t('common.close')}
                    onClick={onClose}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </div>
              </div>
              {skill.description ? (
                <p className="max-w-4xl text-sm text-muted-foreground leading-relaxed">
                  {skill.description}
                </p>
              ) : null}
            </div>

            {/* Source link */}
            {skill.source && skill.sourceUrl ? (
              <a
                href={skill.sourceUrl}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1.5 text-sm text-primary font-medium hover:underline"
              >
                <Link2 className="h-3.5 w-3.5" />
                {skill.source}
              </a>
            ) : null}
            {showCannotCheckStatus ? (
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="outline" className="text-xs text-muted-foreground">
                  {t(resolveUpdateStatusI18nKey(isDeletedUpstream ? 'deletedUpstream' : 'cannotCheck'))}
                </Badge>
                {(() => {
                  const reasonKey = resolveUpdateReasonI18nKey(skill.updateReason);
                  return reasonKey ? (
                    <span className="text-xs text-muted-foreground">
                      {t(reasonKey)}
                    </span>
                  ) : null;
                })()}
              </div>
            ) : null}

            {/* Metadata grid */}
            <div className="grid grid-cols-2 md:grid-cols-3 gap-4 pb-4 border-b border-border">
              {skill.installedAt ? (
                <div className="flex flex-col">
                  <span className="font-heading text-[10px] uppercase font-bold text-muted-foreground tracking-[0.2em]">
                    {t('skills.detail.installed')}
                  </span>
                  <span className="text-sm font-semibold text-accent-foreground mt-1">
                    {formatTime(skill.installedAt, i18n.language)}
                  </span>
                </div>
              ) : null}
              {skill.updatedAt ? (
                <div className="flex flex-col">
                  <span className="font-heading text-[10px] uppercase font-bold text-muted-foreground tracking-[0.2em]">
                    {t('skills.detail.updated')}
                  </span>
                  <span className="text-sm font-semibold text-accent-foreground mt-1">
                    {formatTime(skill.updatedAt, i18n.language)}
                  </span>
                </div>
              ) : null}
              <div className="flex flex-col col-span-2 md:col-span-1">
                <span className="font-heading text-[10px] uppercase font-bold text-muted-foreground tracking-[0.2em]">
                  {t('skills.detail.installPath')}
                </span>
                <div className="flex items-center gap-1 mt-1">
                  <code className="text-sm font-mono text-accent-foreground bg-sidebar px-2 py-1 truncate">
                    {skill.canonicalPath}
                  </code>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button variant="ghost" size="icon-xs" className="h-5 w-5 text-muted-foreground hover:bg-muted/50 border-none shadow-none" onClick={handleCopyPath}>
                        {copied ? (
                          <Check className="h-3 w-3 text-success" />
                        ) : (
                          <Copy className="h-3 w-3" />
                        )}
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent><p>{t('common.copy')}</p></TooltipContent>
                  </Tooltip>
                </div>
              </div>
            </div>

            {/* Agents row */}
            <div className="flex flex-wrap items-start gap-3">
              <span className="font-heading text-[10px] uppercase font-bold text-muted-foreground tracking-[0.2em]">
                {t('skills.detail.agents')}
              </span>
              <div className="flex min-w-0 flex-1 flex-col gap-2">
                {displayAgents.length > 0 ? (
                  <div className="flex flex-wrap items-center gap-2">
                    {displayAgents.map((agentId) => (
                      <span
                        key={agentId}
                        className="inline-flex items-center gap-1 rounded-full bg-primary/10 px-3 py-1 text-[11px] font-bold text-primary"
                      >
                        {agentDisplayNames.get(agentId) ?? agentId}
                      </span>
                    ))}
                  </div>
                ) : (
                  <span className="text-[11px] text-muted-foreground/60">{t('skills.detail.noAgents')}</span>
                )}
                {duplicateCopyCount > 0 ? (
                  <div className="flex items-start gap-1.5 text-xs leading-5 text-amber-700 dark:text-amber-300">
                    <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    <span>
                      {duplicateCopyAgentSummary ? (
                        <>
                          <span className="font-medium">{duplicateCopyAgentSummary}</span>
                          {duplicateCopyAgentSeparator}
                          {t('skills.detail.extraCopiesNamedHint')}
                        </>
                      ) : (
                        t('skills.detail.extraCopiesCountHint', { count: duplicateCopyCount })
                      )}
                    </span>
                  </div>
                ) : null}
              </div>
            </div>

            {/* Markdown 正文 */}
            <div className="pb-10">
              {loading ? (
                <div className="space-y-4">
                  <Skeleton className="h-6 w-1/3" />
                  <Skeleton className="h-4 w-full" />
                  <Skeleton className="h-4 w-5/6" />
                  <Skeleton className="h-4 w-11/12" />
                  <Skeleton className="h-32 w-full mt-6" />
                </div>
              ) : content ? (
                <div className="skill-prose skill-prose-with-lists">
                  <MarkdownContent content={content} />
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center py-20 text-muted-foreground">
                  <p className="text-sm pb-4">{t('skills.detail.emptyContent')}</p>
                  <Button variant="outline" size="sm" onClick={onRetry} className="bg-transparent">
                    <RefreshCw className="h-3.5 w-3.5 mr-2" />
                    {t('skills.detail.retry')}
                  </Button>
                </div>
              )}
            </div>

          </div>
        </ScrollArea>
      </div>
    </div>
  );
});

// Extracted for memo optimization (rerender-memo)
const MarkdownContent = memo(function MarkdownContent({ content }: { content: string }) {
  return <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>;
});

const UpdatingStatusBadge = memo(function UpdatingStatusBadge({
  phase,
}: {
  phase: SkillUpdateActivePhase;
}) {
  const { t } = useTranslation();

  return (
    <Badge variant="outline" className="h-8 px-2 text-xs text-warning animate-pulse">
      <RefreshCw className="h-3.5 w-3.5 animate-spin" />
      {t(resolveSkillUpdatePhaseI18nKey(phase))}
    </Badge>
  );
});
