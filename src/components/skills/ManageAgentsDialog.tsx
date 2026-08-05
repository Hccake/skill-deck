import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
} from 'react';
import { AlertTriangle, Loader2, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type {
  AgentSelectionSubmission,
  InstalledSkill,
  ManageAgentSelectionSnapshot,
  RecoveryAction,
} from '@/bindings';
import { AgentSelectionModeControl } from '@/components/agents/selection/AgentSelectionModeControl';
import { AgentSelectionView } from '@/components/agents/selection/AgentSelectionView';
import { useAgentSelectionPresentation } from '@/components/agents/selection/useAgentSelectionPresentation';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import {
  createAgentSelectionSession,
  hasUserSelectionChanges,
  refreshAgentSelectionSession,
  toggleSelectionGroup,
  toggleInstallOption,
  type AgentSelectionSession,
} from '@/lib/agent-selection-session';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Skeleton } from '@/components/ui/skeleton';
import type { ManageAgentsOutcome } from '@/workflows/skill-manage-agents';

interface ManageAgentsDialogProps {
  skill: InstalledSkill | null;
  snapshot?: ManageAgentSelectionSnapshot | null;
  loading?: boolean;
  loadFailed?: boolean;
  onRetry?: () => void;
  onClose: () => void;
  onSave: (
    selection: AgentSelectionSubmission,
    confirmEntityDirectories?: boolean,
  ) => Promise<ManageAgentsOutcome>;
}

export const ManageAgentsDialog = memo(function ManageAgentsDialog({
  skill,
  snapshot,
  loading = false,
  loadFailed = false,
  onRetry,
  onClose,
  onSave,
}: ManageAgentsDialogProps) {
  const { t } = useTranslation();
  const [saving, setSaving] = useState(false);
  const requestCloseRef = useRef(onClose);
  return (
    <Dialog open={!!skill} onOpenChange={(open) => !open && !saving && requestCloseRef.current()}>
      <DialogContent
        className="grid h-[min(42rem,calc(100dvh-2rem))] w-[calc(100vw-2rem)] min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-3xl"
        dismissible={!saving}
        closeLabel={t('common.close')}
        aria-busy={loading || saving}
      >
        {loading ? (
          <>
            <ManageDialogHeader skillName={skill?.name} />
            <LoadingBody />
          </>
        ) : loadFailed || !snapshot ? (
          <>
            <ManageDialogHeader skillName={skill?.name} />
            <LoadError onRetry={onRetry} />
          </>
        ) : (
          <ReadySession
            skillName={skill?.name}
            snapshot={snapshot}
            saving={saving}
            onSavingChange={setSaving}
            onClose={onClose}
            onSave={onSave}
            requestCloseRef={requestCloseRef}
          />
        )}
      </DialogContent>
    </Dialog>
  );
});

function ReadySession({
  skillName,
  snapshot,
  saving,
  onSavingChange,
  onClose,
  onSave,
  requestCloseRef,
}: {
  skillName?: string;
  snapshot: ManageAgentSelectionSnapshot;
  saving: boolean;
  onSavingChange: (value: boolean) => void;
  onClose: () => void;
  onSave: ManageAgentsDialogProps['onSave'];
  requestCloseRef: MutableRefObject<() => void>;
}) {
  const { t } = useTranslation();
  const presentation = useAgentSelectionPresentation('manage');
  const writeBlocked = useBusinessWriteBlocked();
  const [session, setSession] = useState<AgentSelectionSession>(() => (
    createAgentSelectionSession(snapshot.selection, 'symlink', snapshot.optionStates)
  ));
  const [feedback, setFeedback] = useState<ManageAgentsOutcome['status'] | null>(null);
  const [recovery, setRecovery] = useState<RecoveryAction[]>([]);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const lastRevisionRef = useRef(snapshot.selection.revision);
  useEffect(() => {
    if (lastRevisionRef.current === snapshot.selection.revision) return;
    lastRevisionRef.current = snapshot.selection.revision;
    setSession((current) => refreshAgentSelectionSession(
      current,
      snapshot.selection,
      snapshot.optionStates,
    ));
    setFeedback(null);
  }, [snapshot.optionStates, snapshot.selection]);
  const automaticRepair = useMemo(() => snapshot.optionStates.some((state) => (
    state.initialSelected && state.selectedEffect === 'repair'
  )), [snapshot.optionStates]);
  const userModified = hasUserSelectionChanges(session, snapshot.selection);
  const hasActionableChanges = userModified || automaticRepair;
  const confirmationRequired = feedback === 'confirmationRequired';

  const requestClose = useCallback(() => {
    if (userModified) setConfirmDiscard(true);
    else onClose();
  }, [onClose, userModified]);
  useEffect(() => {
    requestCloseRef.current = requestClose;
    return () => { requestCloseRef.current = onClose; };
  }, [onClose, requestClose, requestCloseRef]);
  const save = async () => {
    setFeedback(null);
    setRecovery([]);
    onSavingChange(true);
    try {
      const outcome = await onSave({
        revision: snapshot.selection.revision,
        selectedOptionIds: session.selectedOptionIds,
        requestedMode: session.mode,
      }, confirmationRequired);
      setFeedback(outcome.status === 'stale' ? null : outcome.status);
      if (outcome.status === 'recoveryRequired') setRecovery(outcome.recovery);
    } catch {
      setFeedback('failed');
    } finally {
      onSavingChange(false);
    }
  };

  return (
    <>
      <ManageDialogHeader skillName={skillName} />
      <div
        data-slot="manage-agents-scroll-content"
        className="min-h-0 overflow-y-auto overflow-x-hidden overscroll-contain px-6 py-5"
      >
        {session.requiresReconfirmation ? (
          <div role="alert" className="mb-4 flex items-center justify-between gap-3 rounded-md border px-3 py-2.5 text-sm">
            <span>{t('agentSelection.selectionChanged')}</span>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="shrink-0"
              onClick={() => setSession((current) => ({ ...current, requiresReconfirmation: false }))}
            >
              {t('agentSelection.confirmCurrentSelection')}
            </Button>
          </div>
        ) : null}
        {feedback && feedback !== 'succeeded' ? (
          <div role="alert" className="mb-4 flex gap-2 rounded-md border border-warning/30 bg-warning/10 p-3 text-sm">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden="true" />
            <span className="space-y-1">
              <span className="block font-medium">{t(`skills.manageAgents.${feedback}`)}</span>
              {feedback === 'failed' ? <span className="block text-muted-foreground">{t('skills.manageAgents.failedDescription')}</span> : null}
              {recovery.map((action) => <RecoveryActions key={action.resourceId} recovery={action} onResolved={onClose} />)}
            </span>
          </div>
        ) : null}
        <div data-slot="agent-selection-mode-bar" className="mb-7 empty:hidden">
          <AgentSelectionModeControl
            snapshot={snapshot.selection}
            session={session}
            onModeChange={(mode) => setSession((current) => ({ ...current, mode }))}
            disabled={saving}
            className="flex-col items-start gap-2"
          />
        </div>
        <AgentSelectionView
          presentation={presentation}
          snapshot={snapshot.selection}
          session={session}
          optionStates={snapshot.optionStates}
          emptyMessage={t('agentSelection.manageEmpty')}
          disabled={saving}
          onOptionChange={(optionId, selected) => setSession((current) => toggleInstallOption(current, snapshot.selection, optionId, selected))}
          onGroupChange={(groupId, selected) => setSession((current) => toggleSelectionGroup(current, snapshot.selection, groupId, selected))}
          onOtherExpandedChange={(otherAgentsExpanded) => setSession((current) => ({ ...current, otherAgentsExpanded }))}
          onAdditionalExpandedChange={(additionalInstallExpanded) => setSession((current) => ({ ...current, additionalInstallExpanded }))}
          onGroupExpandedChange={(groupId, expanded) => setSession((current) => ({
            ...current,
            expandedGroupIds: expanded
              ? [...new Set([...current.expandedGroupIds, groupId])]
              : current.expandedGroupIds.filter((id) => id !== groupId),
          }))}
        />
      </div>
      <DialogFooter className="border-t px-6 py-4">
        {snapshot.selection.installOptions.length === 0 ? (
          <Button onClick={onClose}>{t('common.close')}</Button>
        ) : (
          <>
            {confirmDiscard ? (
              <div role="alert" className="mr-auto text-sm text-muted-foreground">{t('skills.manageAgents.discardConfirm')}</div>
            ) : null}
            <Button variant="outline" onClick={onClose} disabled={saving}>
              {confirmDiscard ? t('skills.manageAgents.discard') : t('common.cancel')}
            </Button>
            {confirmDiscard ? (
              <Button variant="secondary" onClick={() => setConfirmDiscard(false)}>{t('skills.manageAgents.continueEditing')}</Button>
            ) : (
              <Button
                onClick={() => void save()}
                disabled={writeBlocked || saving || session.requiresReconfirmation || !hasActionableChanges}
              >
                {saving ? <Loader2 className="size-3.5 animate-spin" aria-hidden="true" /> : null}
                {confirmationRequired ? t('skills.manageAgents.confirmRemoval') : t('skills.manageAgents.save')}
              </Button>
            )}
          </>
        )}
      </DialogFooter>
    </>
  );
}

function ManageDialogHeader({ skillName }: { skillName?: string }) {
  const { t } = useTranslation();
  const title = t('skills.manageAgents.title', { name: skillName });
  return (
    <DialogHeader className="min-w-0 border-b px-6 py-4 pr-14 text-left">
      <DialogTitle className="min-w-0 truncate" title={title}>
        {title}
      </DialogTitle>
      <DialogDescription className="sr-only">
        {t('skills.manageAgents.description', { name: skillName })}
      </DialogDescription>
    </DialogHeader>
  );
}

function LoadingBody() {
  const { t } = useTranslation();
  return (
    <div role="status" className="min-h-0 space-y-3 overflow-hidden px-6 py-5">
      <span className="sr-only">{t('common.loading')}</span>
      <Skeleton className="h-9 w-full" />
      <Skeleton className="h-10 w-full" />
      <Skeleton className="h-10 w-full" />
      <Skeleton className="h-10 w-3/4" />
    </div>
  );
}

function LoadError({ onRetry }: { onRetry?: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-0 items-center justify-center px-6 py-5">
      <div className="space-y-3 text-center">
        <p role="alert" className="text-sm text-destructive">{t('skills.manageAgents.previewError')}</p>
        <Button onClick={onRetry} disabled={!onRetry}>
          <RefreshCw className="size-3.5" aria-hidden="true" />
          {t('skills.manageAgents.retryPreview')}
        </Button>
      </div>
    </div>
  );
}
