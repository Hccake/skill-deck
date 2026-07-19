// src/components/skills/add-skill/OptionsStep.tsx
import { useEffect, useCallback, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { Checkbox } from '@/components/ui/checkbox';
import {
  listAgents,
  listAgentSelectionGroups,
  listEveInstallTargets,
  getDefaultTargetAgents,
} from '@/hooks/useTauriApi';
import { canCreatePrivateCopy, filterAdditionalAgentIds, migrateDefaultTargetAgents } from '@/lib/agentTargets';
import { agentId, agentsForScope, isAgentDetected } from '@/lib/agents';
import { globalContext } from '@/lib/context';
import { AgentSelector } from '@/components/agents/AgentSelector';
import { getEffectiveInstallMode, shouldShowInstallModeSelection, type WizardState } from './types';
import { Bot, Copy, Info, Link2, type LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { AgentId, AgentSelectionGroups, InstallTargetInfo, ResolvedAgent } from '@/bindings';
import type { AdapterTargetSelection } from '@/lib/install-workflow';
import { useAgentConfigurationFlow } from '@/hooks/useAgentConfigurationFlow';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';

// CLI 默认选中的手动安装目标
const DEFAULT_NON_UNIVERSAL_AGENTS = ['claude-code', 'cursor'];

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
}

export function OptionsStep({ state, updateState }: OptionsStepProps) {
  const { t } = useTranslation();
  const scope = state.scope;
  const [agentInitializationError, setAgentInitializationError] = useState(false);
  const [agentInitializationAttempt, setAgentInitializationAttempt] = useState(0);
  const [selectionGroups, setSelectionGroups] = useState<AgentSelectionGroups>({
    global: [],
    project: [],
  });
  const initializationGeneration = useRef(0);

  // 使用 ref 保存 updateState，避免将其作为 useEffect 依赖（advanced-event-handler-refs）
  const updateStateRef = useRef(updateState);
  useEffect(() => { updateStateRef.current = updateState; });

  // 使用 ref 保存 preSelectedAgents，避免将其作为 useEffect 依赖
  const preSelectedAgentsRef = useRef(state.preSelectedAgents);
  useEffect(() => { preSelectedAgentsRef.current = state.preSelectedAgents; });

  const handleConfiguredAgent = useCallback((runtimeSnapshot: Parameters<typeof agentsForScope>[0], configuredId: AgentId) => {
    const allAgents = agentsForScope(runtimeSnapshot, scope);
    if (!allAgents.some((agent) => agentId(agent) === configuredId)) return;
    updateStateRef.current({
      allAgents,
      selectedAgents: state.selectedAgents.includes(configuredId)
        ? state.selectedAgents
        : [...state.selectedAgents, configuredId],
    });

    const generation = initializationGeneration.current;
    void listAgentSelectionGroups(state.context)
      .then((groups) => {
        if (generation === initializationGeneration.current) {
          setSelectionGroups(groups);
        }
      })
      .catch((error) => {
        console.error('Failed to refresh Agent selection groups:', error);
      });
  }, [scope, state.context, state.selectedAgents]);

  const {
    configuringAgentId,
    configurationResult,
    configure,
  } = useAgentConfigurationFlow({
    context: state.context,
    onSaved: handleConfiguredAgent,
  });

  const unknownAgentIds = useMemo(() => {
    const knownIds = new Set(state.allAgents.map((agent) => agentId(agent)));
    return state.preSelectedAgents.filter((id, index, ids) =>
      ids.indexOf(id) === index && !knownIds.has(id));
  }, [state.allAgents, state.preSelectedAgents]);

  // 初始化 agents 数据 — async-parallel 规则
  useEffect(() => {
    const generation = ++initializationGeneration.current;
    async function initAgents() {
      setAgentInitializationError(false);
      const isProjectScope = scope === 'project';
      const agentsPromise = listAgents(state.context);
      const selectionGroupsPromise = listAgentSelectionGroups(state.context);
      const eveTargetsPromise = isProjectScope
        ? listEveInstallTargets(state.context).catch(() => [])
        : Promise.resolve([] as InstallTargetInfo[]);
      const lastSelectedPromise = Promise.resolve([] as string[]);
      const targetDefaultsPromise = getDefaultTargetAgents(
        globalContext(state.context.environment),
      ).catch(() => null);

      const [runtimeSnapshot, groups, eveTargets, lastSelected, targetDefaults] = await Promise.all([
        agentsPromise,
        selectionGroupsPromise,
        eveTargetsPromise,
        lastSelectedPromise,
        targetDefaultsPromise,
      ]);
      if (generation !== initializationGeneration.current) return;
      setSelectionGroups(groups);
      const allAgents = agentsForScope(runtimeSnapshot, scope);

      let selectedAgents: AgentId[];

      // 优先使用从 CLI 命令解析出的 preSelectedAgents
      if (preSelectedAgentsRef.current.length > 0) {
        const matched = filterAdditionalAgentIds(
          preSelectedAgentsRef.current,
          allAgents,
          scope,
        );
        selectedAgents = matched.length > 0 ? matched : [];
      } else if (targetDefaults) {
        selectedAgents = filterAdditionalAgentIds(
          targetDefaults[scope],
          allAgents,
          scope,
        );
      } else if (lastSelected.length > 0) {
        selectedAgents = migrateDefaultTargetAgents(
          lastSelected,
          allAgents,
        )[scope];
      } else {
        selectedAgents = migrateDefaultTargetAgents(
          DEFAULT_NON_UNIVERSAL_AGENTS,
          allAgents,
        )[scope].filter((id) =>
          allAgents.some((agent) => agentId(agent) === id && isAgentDetected(agent))
        );
      }

      updateStateRef.current({
        allAgents,
        selectedAgents,
        availableAgentTargets: eveTargets,
        selectedAgentTargets: selectedAgents.includes('eve')
          ? eveTargets.map(targetSelection)
          : [],
        privateCopyAgents: [],
      });
    }

    void initAgents().catch(() => {
      if (generation === initializationGeneration.current) {
        setAgentInitializationError(true);
      }
    });
    return () => {
      if (generation === initializationGeneration.current) {
        initializationGeneration.current += 1;
      }
    };
  }, [agentInitializationAttempt, scope, state.projectPath, state.context]);

  const handleSelectionChange = useCallback(
    (agents: string[]) => {
      const next: Partial<WizardState> = { selectedAgents: agents };
      const availableTargets = state.availableAgentTargets ?? [];
      const wasEveSelected = state.selectedAgents.includes('eve');
      const isEveSelected = agents.includes('eve');

      if (!isEveSelected) {
        next.selectedAgentTargets = [];
      } else if (!wasEveSelected && availableTargets.length > 0) {
        next.selectedAgentTargets = availableTargets.map(targetSelection);
      }

      updateState(next);
    },
    [state.availableAgentTargets, state.selectedAgents, updateState]
  );

  const handleAgentTargetChange = useCallback(
    (target: InstallTargetInfo, checked: boolean) => {
      const current = state.selectedAgentTargets ?? [];
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
        state.allAgents.map((agent) => [agentId(agent), agent]),
      );
      const filteredAgents = agents.filter((agentId, index) => {
        if (agents.indexOf(agentId) !== index) return false;
        const agent = agentById.get(agentId);
        return agent ? canCreatePrivateCopy(agent, scope) : false;
      });

      updateState({ privateCopyAgents: filteredAgents });
    },
    [scope, state.allAgents, updateState]
  );

  const shouldShowModeSelection = shouldShowInstallModeSelection(state);
  const effectiveMode = getEffectiveInstallMode(state);
  const availableAgentTargets = state.availableAgentTargets ?? [];
  const selectedTargetKeys = new Set((state.selectedAgentTargets ?? []).map(targetKey));
  const showConcreteTargets = scope === 'project'
    && state.selectedAgents.includes('eve')
    && availableAgentTargets.length > 0;

  return (
    <div className="space-y-6 py-4">
      {/* Agents */}
      <div className="space-y-3">
        <Label className="text-base font-semibold">{t('addSkill.agents.targetTitle')}</Label>
        {agentInitializationError ? (
          <Alert>
            <AlertDescription>
              <p>{t('addSkill.agents.loadError')}</p>
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto p-0"
                onClick={() => setAgentInitializationAttempt((attempt) => attempt + 1)}
              >
                {t('common.retry')}
              </Button>
            </AlertDescription>
          </Alert>
        ) : null}
        <AgentSelector
          selectedAgents={state.selectedAgents}
          privateCopyAgents={state.privateCopyAgents}
          allAgents={state.allAgents}
          selectionGroups={selectionGroups[scope]}
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
