// src/components/skills/SkillDetailPanel.tsx
import { memo, useState, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { Check, X, RefreshCw, Trash2, ArrowUpCircle, Pencil, FolderOutput, Wrench, AlertTriangle, ExternalLink, KeyRound } from 'lucide-react';
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
import {
  CopyablePath,
  DetailBody,
  DetailField,
  DetailSourceLink,
} from '@/components/skills/detail/DetailPrimitives';
import { formatTime } from '@/lib/utils';
import type { InstalledSkill, InstalledSkillLocation, SourceUpdateCheckInfo, UpdateCheckOutcome } from '@/bindings';
import {
  hasIncompleteUpdateCheck,
  resolveEvidenceFailureReasonI18nKey,
  resolveSkillMaintenanceAction,
  isSkillUpdateActive,
  resolveSkillUpdatePhaseI18nKey,
  type SkillUpdateDisplayStatus,
  type SkillUpdateActivePhase,
  resolveUpdateReasonI18nKey,
  resolveUpdateStatusI18nKey,
  providerCooldownDeadline,
  type SkillListItem,
} from '@/stores/skills-utils';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

interface SkillDetailPanelProps {
  skill: SkillListItem;
  /** 当前 Environment 的完整来源诊断，用于 provider 级 cooldown。 */
  sourceDiagnostics?: SourceUpdateCheckInfo[];
  content: string | null;
  loading: boolean;
  agentDisplayNames: Map<string, string>;
  updateStatus?: SkillUpdateDisplayStatus;
  isCheckingUpdates?: boolean;
  projectPath?: string;
  onClose: () => void;
  onCheckUpdates?: () => Promise<UpdateCheckOutcome | null>;
  onUpdate: (name: string, scope: InstalledSkillLocation) => void;
  onDelete: (skill: InstalledSkill) => void;
  onRetry: () => void;
  onManageAgents: (skill: InstalledSkill) => void;
  onCopyToProject?: (skill: InstalledSkill) => void;
  onRepairSource?: (skill: InstalledSkill) => void;
  /** 打开 Git 凭据设置。路由归页面所有，面板保持无路由依赖。 */
  onConfigureGitCredentials?: () => void;
}

export const SkillDetailPanel = memo(function SkillDetailPanel({
  skill,
  sourceDiagnostics = [],
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
  onConfigureGitCredentials,
}: SkillDetailPanelProps) {
  const { t, i18n } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const [checkDone, setCheckDone] = useState(false);
  const hideCheckDoneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const displayAgents = skill.associatedAgents.filter(
    (agentId, index, agents) => agents.indexOf(agentId) === index,
  );
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


  const handleDelete = useCallback(() => {
    onDelete(skill);
  }, [onDelete, skill]);

  const handleUpdate = useCallback(() => {
    onUpdate(skill.name, skill.scope);
  }, [onUpdate, skill.name, skill.scope]);

  const handleManageAgents = useCallback(() => {
    onManageAgents(skill);
  }, [onManageAgents, skill]);

  // WebView 里 <a target="_blank"> 不会交给系统浏览器，外部地址一律走 Tauri opener。
  const handleOpenSource = useCallback((url: string | null | undefined) => {
    if (!url) return;
    void openUrl(url).catch((error: unknown) => {
      console.error('Failed to open Skill source:', error);
      toast.error(t('skills.card.sourceOpenFailed'));
    });
  }, [t]);

  const handleCopyToProject = useCallback(() => {
    onCopyToProject?.(skill);
  }, [onCopyToProject, skill]);

  const handleRepairSource = useCallback(() => {
    onRepairSource?.(skill);
  }, [onRepairSource, skill]);

  const handleCheckUpdates = useCallback(async () => {
    if (!onCheckUpdates) return;

    const outcome = await onCheckUpdates();
    if (outcome !== 'completed') {
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
  }, [onCheckUpdates]);

  const activeUpdatePhase = isSkillUpdateActive(updateStatus) ? updateStatus : null;
  const isUpdateInProgress = activeUpdatePhase !== null;
  const showCheckDone = checkDone && !skill.hasUpdate;
  const typedFailure = skill.updateEvidence?.lastAttempt?.failure ?? null;
  const cooldownDeadline = providerCooldownDeadline([
    ...sourceDiagnostics,
    ...(skill.updateEvidence ? [skill.updateEvidence] : []),
  ]);
  const [cooldownNow, setCooldownNow] = useState(() => Date.now());
  const cooldownActive = cooldownDeadline != null && cooldownDeadline > cooldownNow;
  useEffect(() => {
    if (cooldownDeadline == null) return undefined;
    const timer = setTimeout(
      () => setCooldownNow(Date.now()),
      Math.max(0, cooldownDeadline - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [cooldownDeadline]);
  const isIncompleteCheck = hasIncompleteUpdateCheck(skill);
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
                        aria-label={t('skills.checkCompleted')}
                        title={t('skills.checkCompleted')}
                      >
                        <Check className="h-4 w-4" />
                      </div>
                    ) : (
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 text-muted-foreground hover:text-primary hover:bg-primary/10 cursor-pointer"
                        title={cooldownActive && cooldownDeadline
                          ? t('skills.updateEvidence.retryAt', {
                              time: new Date(cooldownDeadline).toLocaleString(i18n.language),
                            })
                          : t('skills.checkUpdates')}
                        disabled={isCheckingUpdates || cooldownActive}
                        onClick={() => {
                          void handleCheckUpdates();
                        }}
                      >
                        <RefreshCw className={`h-4 w-4 ${isCheckingUpdates ? 'animate-spin motion-reduce:animate-none' : ''}`} />
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
                    title={t('skills.manageAgents.action')}
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
            {skill.source ? (
              <DetailSourceLink label={skill.source} url={skill.sourceUrl} />
            ) : null}
            {showCannotCheckStatus ? (
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="outline" className="text-xs text-muted-foreground">
                  {t(isIncompleteCheck
                    ? 'skills.updateStatusLabel.checkIncomplete'
                    : resolveUpdateStatusI18nKey(isDeletedUpstream ? 'deletedUpstream' : 'cannotCheck'))}
                </Badge>
                {!isIncompleteCheck ? (() => {
                  const reasonKey = resolveUpdateReasonI18nKey(skill.updateReason);
                  return reasonKey ? (
                    <span className="text-xs text-muted-foreground">
                      {t(reasonKey)}
                    </span>
                  ) : null;
                })() : null}
              </div>
            ) : null}
            {isIncompleteCheck && typedFailure ? (
              <div
                role="status"
                className="space-y-2 border-y border-border/60 py-3 text-sm"
              >
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <span className="font-medium text-foreground">
                    {t(resolveEvidenceFailureReasonI18nKey(typedFailure.reason))}
                  </span>
                  {skill.updateEvidence?.checkedAtEpochMs ? (
                    <span className="text-xs text-muted-foreground">
                      {t('skills.updateEvidence.lastChecked', {
                        time: new Date(skill.updateEvidence.checkedAtEpochMs).toLocaleString(i18n.language),
                      })}
                    </span>
                  ) : null}
                  <span className="text-xs text-muted-foreground">
                    {t('skills.updateEvidence.lastAttempt', {
                      time: new Date(skill.updateEvidence?.lastAttempt?.checkedAtEpochMs ?? 0)
                        .toLocaleString(i18n.language),
                    })}
                  </span>
                  {typedFailure.retryAtEpochMs ? (
                    <span className="text-xs text-muted-foreground">
                      {t('skills.updateEvidence.retryAt', {
                        time: new Date(typedFailure.retryAtEpochMs).toLocaleString(i18n.language),
                      })}
                    </span>
                  ) : null}
                </div>
                <div className="flex flex-wrap gap-2">
                  {typedFailure.reason === 'rateLimited'
                    || typedFailure.reason === 'authenticationRequired' ? (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={onConfigureGitCredentials}
                      disabled={!onConfigureGitCredentials}
                    >
                      <KeyRound className="h-3.5 w-3.5" />
                      {t('skills.updateEvidence.actions.configureToken')}
                    </Button>
                  ) : null}
                  {['refNotFound', 'repositoryNotFound', 'notFoundOrUnauthorized'].includes(typedFailure.reason)
                    && skill.sourceUrl ? (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => handleOpenSource(skill.sourceUrl)}
                    >
                      <ExternalLink className="h-3.5 w-3.5" />
                      {t('skills.updateEvidence.actions.openSource')}
                    </Button>
                  ) : null}
                  {!cooldownActive && onCheckUpdates ? (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={isCheckingUpdates}
                      onClick={() => void handleCheckUpdates()}
                    >
                      <RefreshCw className={`h-3.5 w-3.5 ${isCheckingUpdates ? 'animate-spin motion-reduce:animate-none' : ''}`} />
                      {t('skills.updateEvidence.actions.retry')}
                    </Button>
                  ) : null}
                </div>
              </div>
            ) : null}

            {/* Metadata grid */}
            <div className="grid grid-cols-2 md:grid-cols-3 gap-4 pb-4 border-b border-border">
              {skill.installedAt ? (
                <DetailField label={t('skills.detail.installed')}>
                  {formatTime(skill.installedAt, i18n.language)}
                </DetailField>
              ) : null}
              {skill.updatedAt ? (
                <DetailField label={t('skills.detail.updated')}>
                  {formatTime(skill.updatedAt, i18n.language)}
                </DetailField>
              ) : null}
              <DetailField
                label={t('skills.detail.installPath')}
                className="col-span-2 md:col-span-1"
              >
                <CopyablePath value={skill.canonicalPath} />
              </DetailField>
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
              <DetailBody loading={loading} content={content} onRetry={onRetry} />
            </div>

          </div>
        </ScrollArea>
      </div>
    </div>
  );
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
