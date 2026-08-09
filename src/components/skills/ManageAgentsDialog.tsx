import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
} from 'react';
import { AlertTriangle, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type {
  AgentSelectionSubmission,
  InstalledSkill,
  ManageAgentSelectionSnapshot,
  RecoveryAction,
  SkillLocationRef,
} from '@/bindings';
import { AgentSelectionPanel } from '@/components/agents/selection/AgentSelectionPanel';
import {
  useAgentSelectionSession,
  type AgentSelectionSessionController,
  type ManageAgentSelectionSessionRequest,
} from '@/hooks/useAgentSelectionSession';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
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
import type { ManageAgentsOutcome } from '@/workflows/skill-manage-agents';

interface ManageAgentsDialogProps {
  skill: InstalledSkill | null;
  context: SkillLocationRef;
  loadAgentSelection: (
    request: ManageAgentSelectionSessionRequest,
  ) => Promise<ManageAgentSelectionSnapshot>;
  onClose: () => void;
  onSave: (
    selection: AgentSelectionSubmission,
    confirmEntityDirectories?: boolean,
  ) => Promise<ManageAgentsOutcome>;
}

export const ManageAgentsDialog = memo(function ManageAgentsDialog({
  skill,
  context,
  loadAgentSelection,
  onClose,
  onSave,
}: ManageAgentsDialogProps) {
  const { t } = useTranslation();
  const [saving, setSaving] = useState(false);
  const requestCloseRef = useRef(onClose);
  const agentSelectionRequest = useMemo<ManageAgentSelectionSessionRequest>(() => ({
    kind: 'manage',
    context,
    skillName: skill?.name ?? '',
  }), [context, skill?.name]);
  const agentSelection = useAgentSelectionSession({
    active: skill !== null,
    request: agentSelectionRequest,
    load: loadAgentSelection,
  });
  return (
    <Dialog open={!!skill} onOpenChange={(open) => !open && !saving && requestCloseRef.current()}>
      <DialogContent
        className="grid h-[min(42rem,calc(100dvh-2rem))] w-[calc(100vw-2rem)] min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-3xl"
        dismissible={!saving}
        closeLabel={t('common.close')}
        aria-busy={agentSelection.status === 'loading' || saving}
      >
        {agentSelection.status !== 'ready' ? (
          <>
            <ManageDialogHeader skillName={skill?.name} />
            <div className="min-h-0 overflow-y-auto px-6 py-5">
              <AgentSelectionPanel
                usage="manage"
                controller={agentSelection}
                emptyMessage={t('agentSelection.manageEmpty')}
                showUnavailableNotice={false}
              />
            </div>
          </>
        ) : (
          <ReadySession
            skillName={skill?.name}
            agentSelection={agentSelection}
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
  agentSelection,
  saving,
  onSavingChange,
  onClose,
  onSave,
  requestCloseRef,
}: {
  skillName?: string;
  agentSelection: Extract<AgentSelectionSessionController<ManageAgentSelectionSnapshot>, { status: 'ready' }>;
  saving: boolean;
  onSavingChange: (value: boolean) => void;
  onClose: () => void;
  onSave: ManageAgentsDialogProps['onSave'];
  requestCloseRef: MutableRefObject<() => void>;
}) {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const [feedback, setFeedback] = useState<ManageAgentsOutcome['status'] | null>(null);
  const [recovery, setRecovery] = useState<RecoveryAction[]>([]);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const automaticRepair = useMemo(() => agentSelection.optionStates.some((state) => (
    state.initialSelected && state.selectedEffect === 'repair'
  )), [agentSelection.optionStates]);
  const userModified = agentSelection.isDirty;
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
      const outcome = await onSave(agentSelection.submission, confirmationRequired);
      if (outcome.status === 'stale') {
        agentSelection.acceptSnapshot(outcome.snapshot);
        setFeedback(null);
      } else {
        setFeedback(outcome.status);
      }
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
        <AgentSelectionPanel
          usage="manage"
          controller={agentSelection}
          emptyMessage={t('agentSelection.manageEmpty')}
          disabled={saving}
          modeClassName="flex-col items-start gap-2"
          modeWrapperClassName="mb-7"
          showUnavailableNotice={false}
        />
      </div>
      <DialogFooter className="border-t px-6 py-4">
        {agentSelection.selection.installOptions.length === 0 ? (
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
                disabled={writeBlocked || saving || agentSelection.requiresReconfirmation || !hasActionableChanges}
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
