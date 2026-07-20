import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, CheckCircle2, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
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
import {
  fetchAvailable,
  installSkills,
} from '@/hooks/useTauriApi';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useMutationStore } from '@/stores/mutation';
import { appendCrossStorageFailureGuidance } from '@/utils/cross-storage-guidance';
import { formatAppError } from '@/utils/format-app-error';
import type { RepairSourceDraft } from '@/stores/skills-utils';
import type { AppError, FetchResult, InstallResponse } from '@/bindings';
import { prepareInstall } from '@/workflows/skill-install-preparation';

type ValidateState = 'idle' | 'checking' | 'valid' | 'missing' | 'error';
type RepairPhase = 'idle' | 'validating' | 'installing';
type ValidationOwner = 'manual' | 'repair' | null;
interface ValidationResult {
  ok: boolean;
  requiresRiskConfirmation: boolean;
  fetchResult: FetchResult | null;
}

function didRepairInstallSucceed(
  results: InstallResponse,
): boolean {
  return results.units.length > 0
    && results.units.every((unit) => unit.status === 'succeeded');
}

function uniqueAgentIds(agents: string[] | undefined): string[] {
  return Array.from(new Set(agents ?? []));
}

export function RepairSourceDialog() {
  const target = useSkillDialogStore((s) => s.repairSourceTarget);
  const closeRepairSource = useSkillDialogStore((s) => s.closeRepairSource);

  if (!target) return null;

  return (
    <Dialog open={Boolean(target)} onOpenChange={(open) => { if (!open) closeRepairSource(); }}>
      <RepairSourceDialogContent
        key={`${JSON.stringify(target.context)}:${target.scope}:${target.projectPath ?? ''}:${target.skillName}`}
        target={target}
      />
    </Dialog>
  );
}

function RepairSourceDialogContent({ target }: { target: RepairSourceDraft }) {
  const { t } = useTranslation();
  const closeRepairSource = useSkillDialogStore((s) => s.closeRepairSource);
  const markSourceRepairSucceeded = useSkillsDataStore((s) => s.markSourceRepairSucceeded);
  const syncSkills = useSkillsDataStore((s) => s.syncSkills);
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [source, setSource] = useState(target.source);
  const [validateState, setValidateState] = useState<ValidateState>('idle');
  const [validationOwner, setValidationOwner] = useState<ValidationOwner>(null);
  const [riskAcknowledged, setRiskAcknowledged] = useState(false);
  const [requiresRiskConfirmation, setRequiresRiskConfirmation] = useState(false);
  const [repairPhase, setRepairPhase] = useState<RepairPhase>('idle');

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
    setRepairPhase('validating');
    let installCompleted = false;
    try {
      const validation = await validateSource('repair');
      if (!validation.ok || !validation.fetchResult) return;
      if (validation.requiresRiskConfirmation && !riskAcknowledged) return;

      setRepairPhase('installing');
      const targetAgents = uniqueAgentIds(target.privateAdaptedAgents ?? target.agents);
      const targetPrivateCopyAgents = uniqueAgentIds(target.privateCopyAgents);
      const privateCopies = new Set(targetPrivateCopyAgents);
      const agentIntents = uniqueAgentIds([...targetAgents, ...targetPrivateCopyAgents]).map((agentId) => ({
        agentId,
        privateEntry: privateCopies.has(agentId) ? 'optionalSelected' as const : 'required' as const,
        adapterTargets: [],
      }));
      const skill = validation.fetchResult.skills.find((item) => item.name === target.skillName)!;
      const preparation = await prepareInstall({
        context: target.context,
        source: source.trim(),
        discoverySession: validation.fetchResult.discoverySession,
        skillPaths: [skill.relativePath],
        skills: [target.skillName],
        agentIntents,
        requestedMode: 'copy' as const,
        acknowledgeRisk: validation.requiresRiskConfirmation ? riskAcknowledged : true,
      });
      if (preparation.status === 'failed') throw preparation.error;
      const results = await installSkills(
        preparation.prepared.request,
        preparation.prepared.preview.token,
      );
      installCompleted = true;
      if (!didRepairInstallSucceed(results)) {
        toast.error(appendCrossStorageFailureGuidance(
          t('skills.repairSourceDialog.repairFailed'),
          target.context,
          'repair',
          t,
        ));
        return;
      }
      markSourceRepairSucceeded(target.context, target.skillName);
      await syncSkills(target.context);
      closeRepairSource();
    } catch (error) {
      console.error('[RepairSourceDialog] Failed to repair source:', error);
      const message = error && typeof error === 'object' && 'kind' in error
        ? formatAppError(error as AppError, t)
        : t('addSkill.error.unknown');
      toast.error(installCompleted
        ? message
        : appendCrossStorageFailureGuidance(message, target.context, 'repair', t));
    } finally {
      setRepairPhase('idle');
    }
  }, [
    closeRepairSource,
    markSourceRepairSucceeded,
    riskAcknowledged,
    source,
    isWorking,
    syncSkills,
    t,
    target,
    validateSource,
    writeBlocked,
  ]);

  const statusLabel = useMemo(() => {
    if (validateState === 'checking') return t('skills.repairSourceDialog.validating');
    if (validateState === 'missing') return t('skills.repairSourceDialog.sourceMissingSkill', { name: skillName });
    if (validateState === 'error') return t('skills.repairSourceDialog.sourceInvalid');
    if (validateState === 'valid') return t('skills.repairSourceDialog.sourceContainsSkill', { name: skillName });
    return null;
  }, [skillName, t, validateState]);

  return (
      <DialogContent className="gap-0 p-0 sm:max-w-lg">
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
        
        <p className="text-xs leading-5 text-muted-foreground">
          {t('skills.repairSourceDialog.overwriteNotice')}
        </p>
      </div>

      <DialogFooter className="border-t border-border px-6 py-4">
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
  );
}
