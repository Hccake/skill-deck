import { useCallback, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, CheckCircle2, RefreshCw, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { fetchAvailable } from '@/hooks/useTauriApi';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useMutationStore } from '@/stores/mutation';
import type { RepairSourceDraft } from '@/stores/skills-utils';
import type { FetchResult } from '@/bindings';
import { repairSkillSource } from '@/workflows/skill-repair';

type ValidateState = 'idle' | 'checking' | 'valid' | 'missing' | 'error';
type RepairPhase = 'idle' | 'validating' | 'preparing' | 'installing' | 'stopping';
type ValidationOwner = 'manual' | 'repair' | null;
type RepairFeedback = 'failed' | 'stopped' | null;
interface ValidationResult {
  ok: boolean;
  requiresRiskConfirmation: boolean;
  fetchResult: FetchResult | null;
}

export function RepairSourceDialog() {
  const target = useSkillDialogStore((s) => s.repairSourceTarget);

  if (!target) return null;

  return (
    <RepairSourceDialogContent
      key={`${JSON.stringify(target.context)}:${target.scope}:${target.projectPath ?? ''}:${target.skillName}`}
      target={target}
    />
  );
}

function RepairSourceDialogContent({ target }: { target: RepairSourceDraft }) {
  const { t } = useTranslation();
  const closeRepairSource = useSkillDialogStore((s) => s.closeRepairSource);
  const markSourceRepairSucceeded = useSkillsDataStore((s) => s.markSourceRepairSucceeded);
  const syncSkills = useSkillsDataStore((s) => s.syncSkills);
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const cancelActiveMutation = useMutationStore((state) => state.cancelActiveMutation);
  const cancelling = useMutationStore((state) => state.cancelling);
  const [source, setSource] = useState(target.source);
  const [validateState, setValidateState] = useState<ValidateState>('idle');
  const [validationOwner, setValidationOwner] = useState<ValidationOwner>(null);
  const [riskAcknowledged, setRiskAcknowledged] = useState(false);
  const [requiresRiskConfirmation, setRequiresRiskConfirmation] = useState(false);
  const [repairPhase, setRepairPhase] = useState<RepairPhase>('idle');
  const [repairFeedback, setRepairFeedback] = useState<RepairFeedback>(null);
  const [repairErrorMessage, setRepairErrorMessage] = useState<string | null>(null);
  const operationIdRef = useRef<string | null>(null);
  const stopRequestedRef = useRef(false);

  const skillName = target?.skillName ?? '';
  const isChecking = validateState === 'checking';
  const isManualChecking = isChecking && validationOwner === 'manual';
  const isRepairing = repairPhase !== 'idle';
  const isWorking = isChecking || isRepairing;
  const canRepair =
    !writeBlocked
    && !isWorking
    && validateState !== 'missing'
    && (!requiresRiskConfirmation || riskAcknowledged);

  const validateSource = useCallback(async (owner: Exclude<ValidationOwner, null>): Promise<ValidationResult> => {
    if (!source.trim()) {
      setValidateState('error');
      setValidationOwner(null);
      return { ok: false, requiresRiskConfirmation: false, fetchResult: null };
    }

    setValidationOwner(owner);
    setValidateState('checking');
    try {
      const result = await fetchAvailable(target.context, source.trim(), crypto.randomUUID());
      const hasSkill = result.skills.some((skill) => skill.name === target.skillName);
      const nextRequiresRiskConfirmation = result.riskPolicy.kind === 'require-confirmation';
      setRequiresRiskConfirmation(nextRequiresRiskConfirmation);
      setValidateState(hasSkill ? 'valid' : 'missing');
      setValidationOwner(null);
      return {
        ok: hasSkill,
        requiresRiskConfirmation: nextRequiresRiskConfirmation,
        fetchResult: result,
      };
    } catch (error) {
      console.error('[RepairSourceDialog] Failed to validate source:', error);
      setValidateState('error');
      setValidationOwner(null);
      return { ok: false, requiresRiskConfirmation: false, fetchResult: null };
    }
  }, [source, target]);

  const handleValidate = useCallback(async () => {
    if (isWorking) return;
    await validateSource('manual');
  }, [isWorking, validateSource]);

  const handleRepair = useCallback(async () => {
    if (writeBlocked || isWorking) return;
    const operationId = crypto.randomUUID();
    operationIdRef.current = operationId;
    stopRequestedRef.current = false;
    setRepairFeedback(null);
    setRepairErrorMessage(null);
    setRepairPhase('validating');
    try {
      const outcome = await repairSkillSource({
        context: target.context,
        source: source.trim(),
        skillName: target.skillName,
        agents: target.agents,
        privateAdaptedAgents: target.privateAdaptedAgents,
        privateCopyAgents: target.privateCopyAgents,
        acknowledgeRisk: riskAcknowledged,
        operationId,
        stopRequested: () => stopRequestedRef.current || operationIdRef.current !== operationId,
        onPhase: (phase) => setRepairPhase(phase),
      });
      if (operationIdRef.current !== operationId) return;
      if (outcome.status === 'succeeded') {
        markSourceRepairSucceeded(target.context, target.skillName);
        await syncSkills(target.context);
        closeRepairSource();
        return;
      }
      if (outcome.status === 'missing') {
        setValidateState('missing');
      } else if (outcome.status === 'riskRequired') {
        setRequiresRiskConfirmation(true);
      } else if (outcome.status === 'stopped') {
        setRepairFeedback('stopped');
      } else if (outcome.status === 'failed') {
        setRepairFeedback('failed');
        setRepairErrorMessage(null);
      }
    } finally {
      if (operationIdRef.current === operationId) setRepairPhase('idle');
    }
  }, [
    closeRepairSource,
    markSourceRepairSucceeded,
    riskAcknowledged,
    source,
    isWorking,
    syncSkills,
    target,
    writeBlocked,
  ]);

  const handleStop = useCallback(async () => {
    if (!isRepairing || stopRequestedRef.current) return;
    stopRequestedRef.current = true;
    setRepairPhase('stopping');
    if (repairPhase !== 'installing') return;
    try {
      const accepted = await cancelActiveMutation();
      if (!accepted) {
        stopRequestedRef.current = false;
        setRepairPhase('installing');
      }
    } catch {
      stopRequestedRef.current = false;
      setRepairFeedback('failed');
      setRepairErrorMessage(t('skills.repairSourceDialog.stopFailed'));
      setRepairPhase('installing');
    }
  }, [cancelActiveMutation, isRepairing, repairPhase, t]);

  const statusLabel = useMemo(() => {
    if (validateState === 'checking') return t('skills.repairSourceDialog.validating');
    if (validateState === 'missing') return t('skills.repairSourceDialog.sourceMissingSkill', { name: skillName });
    if (validateState === 'error') return t('skills.repairSourceDialog.sourceInvalid');
    if (validateState === 'valid') return t('skills.repairSourceDialog.sourceContainsSkill', { name: skillName });
    return null;
  }, [skillName, t, validateState]);

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !isRepairing) closeRepairSource();
      }}
    >
      <DialogContent
        className="gap-0 p-0 sm:max-w-lg"
        dismissible={!isRepairing}
        aria-busy={isWorking}
        closeLabel={t('common.close')}
      >
      <DialogHeader className="px-6 pb-5 pt-6 pr-12">
        <DialogTitle className="text-xl leading-7">
          {t('skills.repairSourceDialog.title', { name: skillName })}
        </DialogTitle>
        <DialogDescription className="mt-2 text-sm leading-6">
          {t('skills.repairSourceDialog.description', { name: skillName })}
        </DialogDescription>
      </DialogHeader>

      <div className="flex flex-col gap-4 px-6 pb-5">
        <section className="flex items-center gap-3">
          <h3 className="shrink-0 text-sm font-medium leading-5 text-foreground/80">
            {t('skills.repairSourceDialog.sourceLabel')}
          </h3>
          <Input
            aria-label={t('skills.repairSourceDialog.sourceLabel')}
            className="h-9 flex-1 font-mono text-[13px]"
            disabled={isWorking}
            value={source}
            onChange={(event) => {
              setSource(event.target.value);
              setValidateState('idle');
              setRiskAcknowledged(false);
              setRequiresRiskConfirmation(false);
              setRepairFeedback(null);
              setRepairErrorMessage(null);
            }}
          />
        </section>

        {statusLabel ? (
          <div className="flex items-start gap-2 text-sm leading-5 text-muted-foreground">
            {validateState === 'valid' ? <CheckCircle2 className="h-4 w-4 shrink-0 text-success" /> : <AlertTriangle className="h-4 w-4 shrink-0 text-warning" />}
            <span>{statusLabel}</span>
          </div>
        ) : null}

        {requiresRiskConfirmation ? (
          <label className="flex items-start gap-2 text-sm leading-5">
            <Checkbox checked={riskAcknowledged} onCheckedChange={(checked) => setRiskAcknowledged(checked === true)} />
            <span>{t('addSkill.risk.openclawAcknowledge')}</span>
          </label>
        ) : null}

        {repairFeedback ? (
          <p role="alert" className={repairFeedback === 'stopped' ? 'text-sm text-warning' : 'text-sm text-destructive'}>
            {repairFeedback === 'stopped'
              ? t('skills.repairSourceDialog.repairStopped')
              : repairErrorMessage ?? t('skills.repairSourceDialog.repairFailed')}
          </p>
        ) : null}
        
        <p className="text-xs leading-5 text-muted-foreground">
          {t('skills.repairSourceDialog.overwriteNotice')}
        </p>
      </div>

      <DialogFooter className="border-t border-border px-6 py-4">
        {isRepairing ? (
          <Button
            variant="outline"
            onClick={() => void handleStop()}
            disabled={repairPhase === 'stopping' || cancelling}
          >
            <Square className="h-4 w-4" />
            {repairPhase === 'stopping'
              ? t('skills.repairSourceDialog.stopping')
              : t('skills.repairSourceDialog.stop')}
          </Button>
        ) : (
          <Button variant="outline" onClick={() => void handleValidate()} disabled={isWorking}>
          {isManualChecking ? (
            <>
              <RefreshCw className="h-4 w-4 animate-spin" />
              {t('skills.repairSourceDialog.validating')}
            </>
          ) : (
            t('skills.repairSourceDialog.validate')
          )}
          </Button>
        )}
        <Button onClick={() => void handleRepair()} disabled={!canRepair || isWorking}>
          {isRepairing ? (
            <>
              <RefreshCw className="h-4 w-4 animate-spin" />
              {t(repairPhase === 'validating'
                ? 'skills.repairSourceDialog.validating'
                : 'skills.repairSourceDialog.repairing')}
            </>
          ) : (
            t('skills.repairSourceDialog.repair')
          )}
        </Button>
      </DialogFooter>
    </DialogContent>
    </Dialog>
  );
}
