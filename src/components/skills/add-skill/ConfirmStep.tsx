import { useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, Copy, Folder, Link2, Package } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Skeleton } from '@/components/ui/skeleton';
import type { InstallAgentSelectionSnapshot } from '@/bindings';
import type { AgentSelectionSessionController } from '@/hooks/useAgentSelectionSession';
import { getSharedSkillDirectory } from '@/lib/agentTargets';
import { projectAgentSelectionView } from '@/lib/agent-selection-view';
import { prepareInstall } from '@/workflows/skill-install-preparation';
import { formatAppError } from '@/utils/format-app-error';
import type { WizardState } from './types';

interface ConfirmStepProps {
  state: WizardState;
  agentSelection: Extract<
    AgentSelectionSessionController<InstallAgentSelectionSnapshot>,
    { status: 'ready' }
  >;
  updateState: (updates: Partial<WizardState>) => void;
  scope: 'global' | 'project';
  projectPath?: string;
}

export function ConfirmStep({ state, agentSelection, updateState, scope }: ConfirmStepProps) {
  const { t } = useTranslation();
  const updateStateRef = useRef(updateState);
  useEffect(() => { updateStateRef.current = updateState; });
  const requestIdRef = useRef(0);
  const [preparationAttempt, setPreparationAttempt] = useState(0);
  const selection = agentSelection.selection;

  useEffect(() => {
    const requestId = ++requestIdRef.current;
    let cancelled = false;
    if (!state.discoverySession || state.selectedSkills.length === 0) {
      updateStateRef.current({ overwrites: {}, preparation: { status: 'idle' } });
      return;
    }
    const skillPaths = state.selectedSkills.flatMap((name) => {
      const skill = state.availableSkills.find((candidate) => candidate.name === name);
      return skill ? [skill.relativePath] : [];
    });
    updateStateRef.current({ preparation: { status: 'preparing' } });
    void prepareInstall({
      context: state.context,
      source: state.source,
      discoverySession: state.discoverySession,
      skillPaths,
      skills: state.selectedSkills,
      agentSelectionIntent: state.agentSelectionIntent,
      agentSelection: agentSelection.submission,
      acknowledgeRedirect:
        !state.redirectedDownloadHost || state.redirectAcknowledged === true,
    }).then((outcome) => {
      if (cancelled || requestId !== requestIdRef.current) return;
      if (outcome.status === 'selectionStale') {
        agentSelection.acceptSnapshot(outcome.snapshot);
        updateStateRef.current({
          step: 'options',
          preparation: { status: 'idle' },
          overwrites: {},
        });
        return;
      }
      if (outcome.status === 'failed') {
        updateStateRef.current({ preparation: outcome, overwrites: {} });
        return;
      }
      const overwrites = Object.fromEntries(
        outcome.prepared.preview.skills
          .filter((skill) => skill.overwriteTargets.length > 0)
          .map((skill) => [skill.skillName, skill.overwriteTargets]),
      );
      updateStateRef.current({ preparation: outcome, overwrites });
    });
    return () => { cancelled = true; };
  }, [agentSelection, preparationAttempt, selection, state.agentSelectionIntent, state.availableSkills, state.context, state.discoverySession, state.redirectAcknowledged, state.redirectedDownloadHost, state.selectedSkills, state.source]);

  const availableSkillMap = useMemo(
    () => new Map(state.availableSkills.map((skill) => [skill.name, skill])),
    [state.availableSkills],
  );
  const overwriteCount = state.selectedSkills.filter((name) => (state.overwrites[name] ?? []).length > 0).length;
  const blockedSkills = new Set(
    state.preparation.status === 'ready'
      ? state.preparation.prepared.preview.skills
        .filter((skill) => (skill.blockingReasons?.length ?? 0) > 0)
        .map((skill) => skill.skillName)
      : [],
  );
  const projected = selection ? projectAgentSelectionView(selection) : null;
  const directAgents = projected?.directAgents.map((agent) => agent.displayName) ?? [];
  const selectedSet = new Set(agentSelection.submission.selectedOptionIds);
  const selectedOptions = selection?.installOptions.filter((option) => selectedSet.has(option.id)) ?? [];
  const linkOptions = selectedOptions.filter(
    (option) => agentSelection.submission.requestedMode === 'symlink'
      && option.modeConstraint !== 'copyOnly',
  );
  const copyOptions = selectedOptions.filter(
    (option) => agentSelection.submission.requestedMode === 'copy'
      || option.modeConstraint === 'copyOnly',
  );
  const isPreparing = state.preparation.status === 'idle' || state.preparation.status === 'preparing';

  return (
    <div className="space-y-4">
      {state.preparation.status === 'failed' ? (
        <Alert variant="destructive">
          <AlertTriangle />
          <AlertTitle>{t('addSkill.confirm.preparationFailed')}</AlertTitle>
          <AlertDescription>
            <p>{t(`addSkill.confirm.preparationStage.${state.preparation.stage}`)}</p>
            <p>{formatAppError(state.preparation.error, t)}</p>
            <Button type="button" variant="outline" size="sm" className="mt-2" onClick={() => setPreparationAttempt((value) => value + 1)}>
              {t('addSkill.confirm.retryPreparation')}
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      {state.redirectedDownloadHost ? (
        <div className="space-y-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-3">
          <div className="flex items-start gap-2">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden="true" />
            <div className="min-w-0 space-y-1">
              <p className="text-sm font-medium">{t('addSkill.confirm.redirectTitle')}</p>
              <p className="text-sm text-muted-foreground">
                {t('addSkill.confirm.redirectBody', { host: state.redirectedDownloadHost })}
              </p>
            </div>
          </div>
          <label className="flex cursor-pointer items-start gap-2 text-sm">
            <Checkbox
              checked={state.redirectAcknowledged}
              onCheckedChange={(checked) => updateState({ redirectAcknowledged: checked === true })}
              className="mt-0.5"
            />
            <span>{t('addSkill.confirm.redirectAcknowledge')}</span>
          </label>
        </div>
      ) : null}

      <section className="space-y-2">
        <div className="space-y-0.5" data-install-contents-section>
          <h2 className="text-sm font-semibold" data-skill-list-heading>{t('addSkill.confirm.itemsTitle')}</h2>
          {state.preparation.status === 'ready' ? (
            <p className="text-xs leading-5 text-muted-foreground">
              {overwriteCount > 0
                ? t('addSkill.confirm.summary', { count: state.selectedSkills.length, overwriteCount })
                : t('addSkill.confirm.summaryNoOverwrite', { count: state.selectedSkills.length })}
            </p>
          ) : null}
        </div>
        <div className="overflow-hidden rounded-md border bg-card">
          {isPreparing ? state.selectedSkills.map((name) => (
            <div key={name} className="flex items-center justify-between border-b px-3 py-3 last:border-b-0">
              <Skeleton className="h-4 w-32" />
              <Skeleton className="h-5 w-14" />
            </div>
          )) : state.selectedSkills.map((name) => {
            const skill = availableSkillMap.get(name);
            return (
              <div key={name} className="flex min-h-10 items-center gap-3 border-b px-3 py-2.5 last:border-b-0">
                <Package className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="min-w-0 flex-1 truncate font-mono text-[13px]">{name}</span>
                {blockedSkills.has(name) ? (
                  <Badge variant="destructive">{t('addSkill.confirm.directDownloadConflict')}</Badge>
                ) : (state.overwrites[name] ?? []).length > 0 ? (
                  <Badge variant="outline">{t('addSkill.confirm.overwriteGroup')}</Badge>
                ) : null}
                {skill?.digestVerified ? <Badge variant="outline">{t('addSkill.confirm.trust.digestVerified')}</Badge> : null}
              </div>
            );
          })}
        </div>
      </section>

      <section className="space-y-2 pt-2">
        <h2 className="text-sm font-semibold">{t('addSkill.confirm.installPlan')}</h2>
        <div className="space-y-2">
          <PlanRow icon={Folder} title={t('addSkill.confirm.defaultLocation')} path={getSharedSkillDirectory(scope)} names={directAgents} />
          {linkOptions.length > 0 ? (
            <PlanRow icon={Link2} title={t('addSkill.confirm.createLinks')} names={linkOptions.map((option) => option.displayName)} paths={linkOptions.map((option) => option.path)} />
          ) : null}
          {copyOptions.length > 0 ? (
            <PlanRow icon={Copy} title={t('addSkill.confirm.createCopies')} names={copyOptions.map((option) => option.displayName)} paths={copyOptions.map((option) => option.path)} />
          ) : null}
        </div>
      </section>
    </div>
  );
}

function PlanRow({
  icon: Icon,
  title,
  path,
  paths = [],
  names,
}: {
  icon: typeof Folder;
  title: string;
  path?: string;
  paths?: string[];
  names: string[];
}) {
  return (
    <div className="rounded-md border bg-muted/15 px-3 py-2.5">
      <div className="flex items-start gap-2.5">
        <Icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <div className="min-w-0 flex-1 space-y-1.5">
          <div className="flex items-center gap-2">
            <span className="text-[13px] font-semibold">{title}</span>
            {path ? <code className="truncate text-[11px] text-muted-foreground">{path}</code> : null}
          </div>
          {paths.map((item) => <code key={item} className="block truncate text-[11px] text-muted-foreground">{item}</code>)}
          <div className="flex flex-wrap gap-1.5">
            {names.map((name) => <Badge key={name} variant="outline">{name}</Badge>)}
          </div>
        </div>
      </div>
    </div>
  );
}
