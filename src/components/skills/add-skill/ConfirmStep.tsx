import { useEffect, useRef, useState } from 'react';
import { AlertTriangle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { InstallConfirmationSummary } from './InstallConfirmationSummary';
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
    const controller = new AbortController();
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
      signal: controller.signal,
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
      if (controller.signal.aborted || requestId !== requestIdRef.current) return;
      if (outcome.status === 'cancelled') return;
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
    return () => controller.abort();
  }, [agentSelection, preparationAttempt, selection, state.agentSelectionIntent, state.availableSkills, state.context, state.discoverySession, state.redirectAcknowledged, state.redirectedDownloadHost, state.selectedSkills, state.source]);

  const overwriteCount = state.selectedSkills.filter((name) => (state.overwrites[name] ?? []).length > 0).length;
  const libraryOverrideCount = state.preparation.status === 'ready'
    ? state.preparation.prepared.preview.skills.filter((skill) => skill.overridesLibrary).length
    : 0;
  const blockedSkills = new Set(
    state.preparation.status === 'ready'
      ? state.preparation.prepared.preview.skills
        .filter((skill) => (skill.blockingReasons?.length ?? 0) > 0)
        .map((skill) => skill.skillName)
      : [],
  );
  const dependencyOverrides = state.preparation.status === 'ready'
    ? state.preparation.prepared.preview.skills.filter((skill) => skill.replacesDependency && !blockedSkills.has(skill.skillName)).map((skill) => skill.skillName)
    : [];
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
  const requiresSourceRefresh = state.preparation.status === 'failed'
    && ['payloadSessionExpired', 'stalePayload', 'staleEnvironment'].includes(state.preparation.error.kind);
  const retryPreparation = () => {
    if (requiresSourceRefresh) {
      updateState({
        step: 'source',
        fetchStatus: 'idle',
        fetchError: null,
        discoverySession: undefined,
        gitRef: null,
        skillFilter: null,
        sourceSelectionResult: null,
        redirectedDownloadHost: null,
        redirectAcknowledged: false,
        preparation: { status: 'idle' },
        overwrites: {},
      });
      return;
    }
    setPreparationAttempt((value) => value + 1);
  };

  return (
    <div className="space-y-4">
      {state.preparation.status === 'failed' ? (
        <Alert variant="destructive">
          <AlertTriangle />
          <AlertTitle>{t('addSkill.confirm.preparationFailed')}</AlertTitle>
          <AlertDescription>
            <p>{t(`addSkill.confirm.preparationStage.${state.preparation.stage}`)}</p>
            <p>{requiresSourceRefresh ? t('addSkill.confirm.sourceNeedsRefresh') : formatAppError(state.preparation.error, t)}</p>
            <Button type="button" variant="outline" size="sm" className="mt-2" onClick={retryPreparation}>
              {t(requiresSourceRefresh ? 'addSkill.confirm.rediscoverSource' : 'addSkill.confirm.retryPreparation')}
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      {libraryOverrideCount > 0 ? (
        <div className="border-y border-border/70 py-3 text-sm">
          <p className="font-medium">{t('addSkill.confirm.libraryOverrideTitle')}</p>
          <p className="mt-1 text-muted-foreground">{t('addSkill.confirm.libraryOverrideDescription', { count: libraryOverrideCount })}</p>
        </div>
      ) : null}

      {dependencyOverrides.length > 0 ? (
        <div className="border-y border-border/70 py-3 text-sm">
          <p className="font-medium">{t('addSkill.confirm.dependencyOverrideTitle')}</p>
          <p className="mt-1 break-words text-muted-foreground">{dependencyOverrides.join('、')}</p>
          <p className="mt-1 text-muted-foreground">{t('addSkill.confirm.dependencyOverrideDescription')}</p>
        </div>
      ) : null}

      <InstallConfirmationSummary
        state={state} isPreparing={isPreparing} blockedSkills={blockedSkills} overwriteCount={overwriteCount}
        defaultPath={getSharedSkillDirectory(scope)} directAgents={directAgents}
        agentCount={new Set([...(projected?.directAgents ?? []).map(agent => agent.id), ...selectedOptions.flatMap(option => option.agentIds)]).size}
        linkOptions={linkOptions} copyOptions={copyOptions}
        onAcknowledge={redirectAcknowledged => updateState({redirectAcknowledged})}
      />
    </div>
  );
}
