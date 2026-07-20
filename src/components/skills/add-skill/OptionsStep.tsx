// src/components/skills/add-skill/OptionsStep.tsx
import { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { Checkbox } from '@/components/ui/checkbox';
import { canCreatePrivateCopy } from '@/lib/agentTargets';
import { agentId } from '@/lib/agents';
import { AgentSelector } from '@/components/agents/AgentSelector';
import { getEffectiveInstallMode, shouldShowInstallModeSelection, type WizardState } from './types';
import { Bot, Copy, Info, Link2, type LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { AgentId, InstallTargetInfo, ResolvedAgent } from '@/bindings';
import type { AdapterTargetSelection } from '@/lib/install-workflow';
import { useAgentConfigurationFlow } from '@/hooks/useAgentConfigurationFlow';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import type { InstallTargetOptionsController } from '@/hooks/useInstallTargetOptions';

const EMPTY_AGENTS: ResolvedAgent[] = [];

function targetKey(target: InstallTargetInfo | AdapterTargetSelection) {
  return target.targetId;
}

function targetSelection(target: InstallTargetInfo): AdapterTargetSelection {
  return {
    agentId: target.agent,
    targetId: target.targetId,
  };
}

interface OptionsStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState>) => void;
  targetOptions: InstallTargetOptionsController;
}

export function OptionsStep({ state, updateState, targetOptions }: OptionsStepProps) {
  const { t } = useTranslation();
  const scope = state.scope;

  const {
    configuringAgentId,
    configurationResult,
    configure,
  } = useAgentConfigurationFlow({
    context: state.context,
    onSaved: targetOptions.acceptConfiguredAgent,
  });

  const facts = targetOptions.status === 'ready' ? targetOptions.facts : null;
  const allAgents = facts?.allAgents ?? EMPTY_AGENTS;

  const unknownAgentIds = useMemo(() => {
    if (!facts) return [];
    const knownIds = new Set(allAgents.map((agent) => agentId(agent)));
    return state.preSelectedAgents.filter((id, index, ids) =>
      ids.indexOf(id) === index && !knownIds.has(id));
  }, [allAgents, facts, state.preSelectedAgents]);

  const handleSelectionChange = useCallback(
    (agents: string[]) => {
      const next: Partial<WizardState> = { selectedAgents: agents };
      const availableTargets = facts?.availableAgentTargets ?? [];
      const wasEveSelected = state.selectedAgents.includes('eve');
      const isEveSelected = agents.includes('eve');

      if (!isEveSelected) {
        next.selectedAgentTargets = [];
      } else if (!wasEveSelected && availableTargets.length > 0) {
        next.selectedAgentTargets = availableTargets.map(targetSelection);
      }

      updateState(next);
    },
    [facts, state.selectedAgents, updateState]
  );

  const handleAgentTargetChange = useCallback(
    (target: InstallTargetInfo, checked: boolean) => {
      const current = state.selectedAgentTargets;
      const key = targetKey(target);
      const nextTargets = checked
        ? [...current.filter((item) => targetKey(item) !== key), targetSelection(target)]
        : current.filter((item) => targetKey(item) !== key);
      const nextAgents = nextTargets.length > 0 && !state.selectedAgents.includes(target.agent)
        ? [...state.selectedAgents, target.agent]
        : nextTargets.length === 0
          ? state.selectedAgents.filter((agent) => agent !== target.agent)
          : state.selectedAgents;

      updateState({
        selectedAgentTargets: nextTargets,
        selectedAgents: nextAgents,
      });
    },
    [state.selectedAgentTargets, state.selectedAgents, updateState],
  );

  const handlePrivateCopyChange = useCallback(
    (agents: string[]) => {
      const agentById = new Map<AgentId, ResolvedAgent>(
        allAgents.map((agent) => [agentId(agent), agent]),
      );
      const filteredAgents = agents.filter((agentId, index) => {
        if (agents.indexOf(agentId) !== index) return false;
        const agent = agentById.get(agentId);
        return agent ? canCreatePrivateCopy(agent, scope) : false;
      });

      updateState({ privateCopyAgents: filteredAgents });
    },
    [allAgents, scope, updateState]
  );

  const targetPresentationState = { ...state, allAgents };
  const shouldShowModeSelection = shouldShowInstallModeSelection(targetPresentationState);
  const effectiveMode = getEffectiveInstallMode(targetPresentationState);
  const availableAgentTargets = facts?.availableAgentTargets ?? [];
  const selectedTargetKeys = new Set(state.selectedAgentTargets.map(targetKey));
  const showConcreteTargets = scope === 'project'
    && state.selectedAgents.includes('eve')
    && availableAgentTargets.length > 0;

  if (!facts) {
    return (
      <div className="space-y-3 py-4">
        <Label className="text-base font-semibold">{t('addSkill.agents.targetTitle')}</Label>
        {targetOptions.status === 'error' ? (
          <Alert>
            <AlertDescription>
              <p>{t('addSkill.agents.loadError')}</p>
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto p-0"
                onClick={() => void targetOptions.retry()}
              >
                {t('common.retry')}
              </Button>
            </AlertDescription>
          </Alert>
        ) : (
          <p role="status" className="text-sm text-muted-foreground">
            {t('common.loading')}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-6 py-4">
      {/* Agents */}
      <div className="space-y-3">
        <Label className="text-base font-semibold">{t('addSkill.agents.targetTitle')}</Label>
        {facts.defaultsUnavailable ? (
          <Alert>
            <AlertDescription>{t('addSkill.agents.defaultLoadWarning')}</AlertDescription>
          </Alert>
        ) : null}
        <AgentSelector
          selectedAgents={state.selectedAgents}
          privateCopyAgents={state.privateCopyAgents}
          allAgents={allAgents}
          selectionGroups={facts.selectionGroups}
          onSelectionChange={handleSelectionChange}
          onPrivateCopyChange={handlePrivateCopyChange}
          scope={state.scope}
          privateCopyAgentsExpanded={state.privateCopyAgentsExpanded}
          onPrivateCopyExpandedChange={(expanded) => updateState({ privateCopyAgentsExpanded: expanded })}
          unknownAgentIds={unknownAgentIds}
          configuringAgentId={configuringAgentId}
          onConfigureAgent={(id) => void configure(id)}
        />
        {configurationResult ? (
          <p role="status" aria-live="polite" className="text-xs text-muted-foreground">
            {t(`addSkill.agents.configurationResult.${configurationResult}`)}
          </p>
        ) : null}
      </div>

      {showConcreteTargets ? (
        <div className="space-y-3">
          <div className="space-y-0.5">
            <Label className="text-base font-semibold">{t('addSkill.agents.concreteTargetsTitle')}</Label>
            <p className="text-xs leading-5 text-muted-foreground">
              {t('addSkill.agents.concreteTargetsHint')}
            </p>
          </div>
          <div className="space-y-1.5">
            {availableAgentTargets.map((target) => {
              const id = `agent-target-${target.targetId}`;
              return (
                <div key={target.targetId} className="flex items-start gap-2 rounded-md border border-border/50 bg-muted/15 px-3 py-2">
                  <Checkbox
                    id={id}
                    checked={selectedTargetKeys.has(targetKey(target))}
                    onCheckedChange={(checked) => handleAgentTargetChange(target, checked === true)}
                    className="mt-0.5"
                  />
                  <Label htmlFor={id} className="min-w-0 flex-1 cursor-pointer space-y-0.5">
                    <span className="flex items-center gap-1.5 text-sm font-medium">
                      <Bot className="h-3.5 w-3.5 text-muted-foreground" />
                      {target.displayName}
                    </span>
                    <span className="block truncate font-mono text-[11px] text-muted-foreground">
                      {target.path}
                    </span>
                  </Label>
                </div>
              );
            })}
          </div>
        </div>
      ) : null}

      {/* Mode */}
      {shouldShowModeSelection ? (
        <div className="space-y-3">
          <Label className="text-base font-semibold">{t('addSkill.mode.title')}</Label>
          <RadioGroup
            value={effectiveMode}
            onValueChange={(value) =>
              updateState({ mode: value as 'symlink' | 'copy' })
            }
            className="grid grid-cols-2 gap-2"
          >
            <InstallModeOption
              value="symlink"
              selected={effectiveMode === 'symlink'}
              icon={Link2}
              title={t('addSkill.mode.symlink')}
              description={t('addSkill.mode.symlinkHint')}
              badge={t('addSkill.mode.recommended')}
            />
            <InstallModeOption
              value="copy"
              selected={effectiveMode === 'copy'}
              icon={Copy}
              title={t('addSkill.mode.copy')}
              description={t('addSkill.mode.copyHint')}
            />
          </RadioGroup>
        </div>
      ) : (
        <div className="space-y-3">
          <Label className="text-base font-semibold">{t('addSkill.mode.title')}</Label>
          <div className="flex items-start gap-3 rounded-lg border border-border/50 bg-muted/20 px-3.5 py-3">
            <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border/50 bg-background text-muted-foreground">
              <Info className="h-3.5 w-3.5" />
            </span>
            <div className="min-w-0 space-y-1">
              <p className="text-[13px] font-medium text-foreground">
                {t('addSkill.mode.singleDirectoryTitle')}
              </p>
              <p className="text-[13px] text-muted-foreground leading-relaxed">
                {t('addSkill.mode.singleDirectoryHint')}
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

interface InstallModeOptionProps {
  value: 'symlink' | 'copy';
  selected: boolean;
  icon: LucideIcon;
  title: string;
  description: string;
  badge?: string;
}

function InstallModeOption({
  value,
  selected,
  icon: Icon,
  title,
  description,
  badge,
}: InstallModeOptionProps) {
  const id = `mode-${value}`;

  return (
    <div className="relative min-w-0">
      <Label
        htmlFor={id}
        className={cn(
          'flex h-full min-h-[68px] cursor-pointer items-start gap-3 rounded-lg border px-3.5 py-3 text-left transition-colors duration-200',
          selected
            ? 'border-primary/60 bg-primary/5'
            : 'border-border/50 bg-background hover:bg-muted/30 hover:border-border shadow-sm'
        )}
      >
        <RadioGroupItem
          value={value}
          id={id}
          className="mt-0.5 shrink-0"
        />

        <div className="min-w-0 flex-1 space-y-1.5">
          <div className="flex min-w-0 items-center gap-2">
            <Icon className={cn("h-4 w-4 shrink-0 focus:outline-none", selected ? "text-primary" : "text-muted-foreground")} />
            <span className={cn('truncate text-[13px] font-medium leading-none', selected ? 'text-foreground' : 'text-foreground/90')}>
              {title}
            </span>
            {badge ? (
              <span className="shrink-0 rounded border border-border/50 bg-muted/50 px-1.5 py-0.5 text-[10px] font-medium leading-none text-muted-foreground delay-0">
                {badge}
              </span>
            ) : null}
          </div>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {description}
          </p>
        </div>
      </Label>
    </div>
  );
}
