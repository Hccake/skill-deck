// src/components/skills/SkillDetailPanel.tsx
import { memo, useState, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { formatAppError } from '@/utils/format-app-error';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { Check, X, RefreshCw, Trash2, ArrowUpCircle, Pencil, FolderOutput, AlertTriangle, ExternalLink, KeyRound } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  DetailBody,
  DetailAction,
  DetailDescription,
  DetailField,
  DetailSourceLink,
} from '@/components/skills/detail/DetailPrimitives';
import { formatTime } from '@/lib/utils';
import type { InstalledLibraryVersion, InstalledSkill, InstalledSkillLocation, ScopePathBase, SkillLocationRef, UpdateCheckOutcome } from '@/bindings';
import { InstallPath } from './InstallPath';
import { SkillAgentBadge, SkillCardAttentionRow, SkillCardStatusLabel } from './card/SkillCardPrimitives';
import { skillStatusPresentation } from '@/lib/skill-status-presentation';
import {
  hasIncompleteUpdateCheck,
  isSkillUpdateActive,
  resolveSkillUpdatePhaseI18nKey,
  type SkillUpdateDisplayStatus,
  type SkillUpdateActivePhase,
  providerCooldownDeadline,
  type SkillListItem,
} from '@/stores/skills-utils';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

interface SkillDetailPanelProps {
  skill: SkillListItem;
  content: string | null;
  loading: boolean;
  agentDisplayNames: Map<string, string>;
  updateStatus?: SkillUpdateDisplayStatus;
  isCheckingUpdates?: boolean;
  context?: SkillLocationRef;
  pathBase?: ScopePathBase | null;
  onClose: () => void;
  onCheckUpdates?: () => Promise<UpdateCheckOutcome | null>;
  onUpdate: (name: string, scope: InstalledSkillLocation) => void;
  onDelete: (skill: InstalledSkill) => void;
  onRetry: () => void;
  onManageAgents: (skill: InstalledSkill) => void;
  onCopyToProject?: (skill: InstalledSkill) => void;
  /** 打开 Git 凭据设置。路由归页面所有，面板保持无路由依赖。 */
  onConfigureGitCredentials?: () => void;
  onOpenLibraryVersion?: (version: InstalledLibraryVersion) => void;
}

export const SkillDetailPanel = memo(function SkillDetailPanel({
  skill,
  content,
  loading,
  agentDisplayNames,
  updateStatus,
  isCheckingUpdates = false,
  context,
  pathBase,
  onClose,
  onCheckUpdates,
  onUpdate,
  onDelete,
  onRetry,
  onManageAgents,
  onCopyToProject,
  onConfigureGitCredentials,
  onOpenLibraryVersion,
}: SkillDetailPanelProps) {
  const { t, i18n } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked() || Boolean(skill.maintenanceError);
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
  const showCheckDone = checkDone && !skill.hasUpdate && skill.updateStatus === 'upToDate';
  const typedFailure = skill.updateEvidence?.lastAttempt?.failure ?? null;
  const cooldownDeadline = providerCooldownDeadline([
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
  const presentation = skillStatusPresentation(skill);
  const notice = presentation.notice;
  const noticeDescription = notice?.error ? formatAppError(notice.error, t)
    : notice?.hintKey ? t(notice.hintKey) : undefined;
  const canShowUpdateAction = (skill.canRunUpdate === true || (skill.hasUpdate === true && skill.canRunUpdate !== false)) && !isDeletedUpstream;
  const canConfigureCredentials = isIncompleteCheck && (typedFailure?.reason === 'rateLimited' || typedFailure?.reason === 'authenticationRequired');
  const canInspectSource = isIncompleteCheck && typedFailure != null
    && ['refNotFound', 'repositoryNotFound', 'notFoundOrUnauthorized'].includes(typedFailure.reason)
    && Boolean(skill.sourceUrl);
  return (
    <div className="skill-detail-layout h-full flex flex-col overflow-hidden bg-surface">
      <div className="flex-1 min-h-0 relative">
        <ScrollArea className="absolute inset-0 w-full h-full">
          <div className="skill-detail-content w-full space-y-4">

            <div className="space-y-2">
              <div className="flex flex-wrap justify-between items-start gap-x-4 gap-y-2">
                <div className="flex min-w-0 flex-auto flex-wrap items-center gap-2">
                  <h2 className="min-w-0 text-xl font-heading font-semibold text-foreground leading-7 [overflow-wrap:anywhere]">
                    {skill.name}
                  </h2>
                  {presentation.available && !updateStatus ? <SkillCardStatusLabel label={t('skills.updateStatusLabel.available')} /> : null}
                </div>
                <div className="ml-auto flex max-w-full shrink-0 flex-wrap justify-end gap-1">
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
                      <DetailAction
                          className="text-primary hover:text-primary hover:bg-primary/10"
                          label={t('skills.actions.update')}
                          disabled={writeBlocked}
                          onClick={handleUpdate}
                      >
                        <ArrowUpCircle className="h-4 w-4" aria-hidden="true" />
                      </DetailAction>
                  ) : null}
                  {!isUpdateInProgress && onCheckUpdates && skill.canCheckForUpdates === true ? (
                    showCheckDone ? (
                      <div
                        className="flex h-8 w-8 items-center justify-center text-success"
                        aria-label={t('skills.checkUpToDate')}
                        title={t('skills.checkUpToDate')}
                      >
                        <Check className="h-4 w-4" />
                      </div>
                    ) : (
                      <DetailAction
                        className="hover:text-primary hover:bg-primary/10"
                        label={t('skills.checkUpdates')}
                        hint={cooldownActive && cooldownDeadline
                          ? t('skills.updateEvidence.retryAt', {
                              time: new Date(cooldownDeadline).toLocaleString(i18n.language),
                            })
                          : t(isIncompleteCheck ? 'skills.detail.retryCheckHint' : 'skills.checkUpdates')}
                        disabled={isCheckingUpdates || cooldownActive}
                        onClick={() => {
                          void handleCheckUpdates();
                        }}
                      >
                        <RefreshCw className={`h-4 w-4 ${isCheckingUpdates ? 'animate-spin motion-reduce:animate-none' : ''}`} aria-hidden="true" />
                      </DetailAction>
                    )
                  ) : null}
                  {skill.scope === 'project' && onCopyToProject ? (
                    <DetailAction
                      className="hover:text-primary hover:bg-primary/10"
                      label={t('skills.actions.copyToProject')}
                      disabled={writeBlocked}
                      onClick={handleCopyToProject}
                    >
                      <FolderOutput className="h-4 w-4" aria-hidden="true" />
                    </DetailAction>
                  ) : null}
                  <DetailAction
                    label={t('skills.manageAgents.action')}
                    disabled={writeBlocked}
                    onClick={handleManageAgents}
                  >
                    <Pencil className="h-4 w-4" aria-hidden="true" />
                  </DetailAction>
                  <DetailAction
                    className="hover:text-destructive hover:bg-destructive/10"
                    label={t('skills.actions.delete')}
                    disabled={writeBlocked}
                    onClick={handleDelete}
                  >
                    <Trash2 className="h-4 w-4" aria-hidden="true" />
                  </DetailAction>
                  <DetailAction
                    className="ml-1"
                    label={t('common.close')}
                    onClick={onClose}
                  >
                    <X className="h-4 w-4" aria-hidden="true" />
                  </DetailAction>
                </div>
              </div>
              {skill.description ? (
                <DetailDescription key={skill.name}>{skill.description}</DetailDescription>
              ) : null}
              {skill.maintenanceError ? (
                <p role="alert" className="rounded-md border border-warning/40 bg-warning/10 p-3 text-sm">
                  {formatAppError(skill.maintenanceError, t)}
                </p>
              ) : null}
            </div>

            <div className="space-y-3">
              <div className="skill-detail-metadata">
                {presentation.local || presentation.sourceLabel ? (
                  <DetailField label={t('skills.detail.source')}>
                    {presentation.local ? t('skills.updateStatusLabel.localSource') : (
                      <DetailSourceLink label={presentation.sourceLabel!} url={skill.sourceUrl}
                        hint={presentation.sourceHintKey ? t(presentation.sourceHintKey) : undefined} />
                    )}
                  </DetailField>
                ) : null}
                {skill.installedAt || skill.updatedAt ? (
                  <div className="skill-detail-dates">
                    {skill.installedAt ? (
                      <DetailField label={t('skills.detail.installed')}>
                        <time dateTime={skill.installedAt}>{formatTime(skill.installedAt, i18n.language)}</time>
                      </DetailField>
                    ) : null}
                    {skill.updatedAt ? (
                      <DetailField label={t('skills.detail.updated')}>
                        <time dateTime={skill.updatedAt}>{formatTime(skill.updatedAt, i18n.language)}</time>
                      </DetailField>
                    ) : null}
                  </div>
                ) : null}
                <DetailField label={t('skills.detail.installPath')} className="skill-detail-path">
                  <InstallPath
                    path={{ environment: context?.environment ?? pathBase?.logicalRoot.environment ?? { kind: 'native' }, nativePath: skill.canonicalPath }}
                    base={pathBase}
                    scope={skill.scope}
                    className="h-5 text-foreground"
                  />
                </DetailField>
              </div>

              {notice || canConfigureCredentials || canInspectSource ? (
                <div
                  role="status"
                  className="space-y-2 text-sm"
                >
                  {notice ? <SkillCardAttentionRow labels={[t(notice.labelKey)]} description={noticeDescription} /> : null}
                  {canConfigureCredentials || canInspectSource ? (
                    <div className="flex flex-wrap gap-2">
                      {canConfigureCredentials ? (
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
                      {canInspectSource ? (
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
                    </div>
                  ) : null}
                </div>
              ) : null}

              {skill.libraryVersions?.length ? (
                <DetailField label={t('skills.detail.libraryVersions')}>
                  <div className="flex flex-col items-start gap-1">
                    {skill.libraryVersions.map((version) => (
                      <Button
                        key={`${version.libraryId}:${version.skillName}`}
                        variant="link" className="h-auto max-w-full justify-start whitespace-normal p-0 text-left text-[13px] leading-5 has-[>svg]:px-0 [overflow-wrap:anywhere]"
                        onClick={() => onOpenLibraryVersion?.(version)}
                        disabled={!onOpenLibraryVersion}
                      >
                        {version.libraryName} · {version.skillName}
                        <ExternalLink className="h-3 w-3 shrink-0" aria-hidden="true" />
                      </Button>
                    ))}
                  </div>
                </DetailField>
              ) : null}

              {/* Agents row */}
              <DetailField label={t('skills.detail.agents')}>
                <div className="flex min-w-0 flex-1 flex-col gap-2">
                  {displayAgents.length > 0 ? (
                    <div className="flex flex-wrap items-start gap-x-1.5 gap-y-1">
                      {displayAgents.map((agentId) => (
                        <SkillAgentBadge key={agentId}>
                          {agentDisplayNames.get(agentId) ?? agentId}
                        </SkillAgentBadge>
                      ))}
                    </div>
                  ) : (
                    <span className="text-[13px] text-muted-foreground">{t('skills.detail.noAgents')}</span>
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
              </DetailField>

            </div>

            {/* Markdown 正文 */}
            <div className="border-t border-border/70 pt-4 pb-8">
              {!skill.maintenanceError ? <DetailBody loading={loading} content={content} onRetry={onRetry} /> : null}
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
