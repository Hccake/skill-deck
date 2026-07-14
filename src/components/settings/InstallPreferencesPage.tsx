import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import { Checkbox } from '@/components/ui/checkbox';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { AgentIcon } from '@/components/ui/agent-icon';
import { cn } from '@/lib/utils';
import {
  formatAgentTargetPath,
  getAgentTarget,
  getSharedSkillDirectory,
  groupAgentsByScopedTarget,
  type InstallScope,
} from '@/lib/agentTargets';
import { useSettingsStore } from '@/stores/settings';
import { useMutationStore } from '@/stores/mutation';
import type { AgentInfo } from '@/hooks/useTauriApi';
import type { EnvironmentRef } from '@/bindings';
import type { AgentDefaultsSnapshot } from '@/stores/settings';

interface ScopeAgentGroups {
  detectedDefaultAvailable: AgentInfo[];
  undetectedDefaultAvailable: AgentInfo[];
  detectedPrivateRequired: AgentInfo[];
  visiblePrivateRequiredAgents: AgentInfo[];
  hiddenPrivateRequiredAgents: AgentInfo[];
  selectableCount: number;
  isAllSelected: boolean;
}

export function InstallPreferencesPage({
  environment,
  snapshot,
}: {
  environment: EnvironmentRef;
  snapshot: AgentDefaultsSnapshot;
}) {
  const { t } = useTranslation();
  const mutationActive = useMutationStore((state) => state.activeMutation !== null);
  const saveAgentDefaults = useSettingsStore((state) => state.saveAgentDefaults);
  const writeBlocked = mutationActive || snapshot.loadState !== 'ready' || snapshot.saving;
  const loaded = snapshot.agents.length > 0 || snapshot.loadState === 'ready';

  const saveScope = (scope: InstallScope, agents: string[]) => {
    void saveAgentDefaults(environment, {
      ...snapshot.defaults,
      [scope]: agents,
    });
  };

  return (
    <div className="space-y-5 pb-8">
      <header className="max-w-3xl space-y-1.5">
        <div className="flex items-center gap-2">
          <h2 className="text-lg font-semibold tracking-tight text-foreground">
            {t('settings.installPreferences.title')}
          </h2>
        </div>
        <p className="text-sm leading-6 text-muted-foreground">
          {t('settings.installPreferences.description')}
        </p>
      </header>

      <Tabs defaultValue="global" className="max-w-3xl">
        <TabsList className="grid w-full max-w-xs grid-cols-2">
          <TabsTrigger value="global">{t('settings.installPreferences.globalTitle')}</TabsTrigger>
          <TabsTrigger value="project">{t('settings.installPreferences.projectTitle')}</TabsTrigger>
        </TabsList>
        {(['global', 'project'] as const).map((scope) => (
          <TabsContent key={scope} value={scope} className="mt-0 focus-visible:outline-none focus-visible:ring-0">
            <ScopePreferencePanel
              scope={scope}
              agents={snapshot.agents}
              selectedAgents={snapshot.defaults[scope]}
              loaded={loaded}
              writeBlocked={writeBlocked}
              onToggle={(agentId) => {
                const selected = snapshot.defaults[scope];
                saveScope(
                  scope,
                  selected.includes(agentId)
                    ? selected.filter((id) => id !== agentId)
                    : [...selected, agentId],
                );
              }}
              onSelectAll={(agents) => saveScope(scope, agents)}
            />
          </TabsContent>
        ))}
      </Tabs>
    </div>
  );
}

function ScopePreferencePanel({
  scope,
  agents,
  selectedAgents,
  loaded,
  writeBlocked,
  onToggle,
  onSelectAll,
}: {
  scope: InstallScope;
  agents: AgentInfo[];
  selectedAgents: string[];
  loaded: boolean;
  writeBlocked: boolean;
  onToggle: (agentId: string) => void;
  onSelectAll: (agents: string[]) => void;
}) {
  const { t } = useTranslation();
  const selectedAgentIds = useMemo(() => new Set(selectedAgents), [selectedAgents]);
  const agentGroups = useMemo<ScopeAgentGroups>(() => {
    const groups = groupAgentsByScopedTarget(agents, scope, selectedAgentIds);

    return {
      ...groups,
      isAllSelected: groups.detectedPrivateRequired.length > 0
        && groups.detectedPrivateRequired.every((agent) => selectedAgentIds.has(agent.id)),
    };
  }, [agents, scope, selectedAgentIds]);

  if (!loaded) {
    return <LoadingRows />;
  }

  const {
    detectedDefaultAvailable,
    undetectedDefaultAvailable,
    detectedPrivateRequired,
    visiblePrivateRequiredAgents,
    hiddenPrivateRequiredAgents,
    selectableCount,
    isAllSelected,
  } = agentGroups;

  return (
    <div className="space-y-3">
      <section className="rounded-lg border border-border/60 bg-background p-3.5">
        <div className="mb-3 space-y-1">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0 space-y-1">
              <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                <h3 className="text-[13px] font-semibold tracking-tight text-foreground">
                  {t('settings.installPreferences.automaticSection')}
                </h3>
                <span className="text-xs leading-5 text-muted-foreground">
                  {t('settings.installPreferences.automaticHint', { path: getSharedSkillDirectory(scope) })}
                </span>
              </div>
            </div>
            <span className="shrink-0 text-xs text-muted-foreground">
              {t('settings.installPreferences.automaticDetectionSummary', {
                detected: detectedDefaultAvailable.length,
                undetected: undetectedDefaultAvailable.length,
              })}
            </span>
          </div>
        </div>

        <div className="space-y-3">
          <div className="flex flex-wrap gap-2">
            {detectedDefaultAvailable.map((agent) => (
              <div key={agent.id} className="flex items-center gap-1.5 rounded border border-border/40 bg-muted/10 px-2 py-1">
                <AgentIcon agentId={agent.id} className="h-4 w-4 bg-transparent border-0" iconClassName="h-3.5 w-3.5 text-foreground/80" />
                <span className="text-xs font-medium text-foreground">{agent.name}</span>
              </div>
            ))}
            {detectedDefaultAvailable.length === 0 && (
              <span className="text-xs text-muted-foreground">
                {t('settings.installPreferences.noAutomaticAgents')}
              </span>
            )}
          </div>

          {undetectedDefaultAvailable.length > 0 && (
            <Collapsible>
              <CollapsibleTrigger className="group flex items-center text-xs text-muted-foreground transition-colors hover:text-foreground">
                {t('settings.installPreferences.otherAutomaticAgentsToggle', { count: undetectedDefaultAvailable.length })}
                <ChevronDown className="ml-1 h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180" />
              </CollapsibleTrigger>
              <CollapsibleContent className="mt-2 flex flex-wrap gap-2">
                {undetectedDefaultAvailable.map((agent) => (
                  <div key={agent.id} className="flex items-center gap-1.5 rounded border border-border/40 bg-muted/5 px-2 py-1 opacity-70">
                    <AgentIcon agentId={agent.id} className="h-4 w-4 bg-transparent border-0" iconClassName="h-3.5 w-3.5 text-muted-foreground/80" />
                    <span className="text-xs font-medium text-muted-foreground">{agent.name}</span>
                  </div>
                ))}
              </CollapsibleContent>
            </Collapsible>
          )}
        </div>
      </section>

      <section className="rounded-lg border border-border/60 bg-background p-3.5">
        <div className="mb-3">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <h3 className="text-[13px] font-semibold tracking-tight text-foreground">
                {t('settings.installPreferences.additionalSection')}
              </h3>
              <p className="mt-1 text-xs leading-5 text-muted-foreground">
                {t('settings.installPreferences.additionalHint')}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-3">
              <span className="text-xs text-muted-foreground">
                {t('settings.installPreferences.selectedCount', { count: selectedAgents.length })}
              </span>
              <label className="flex cursor-pointer items-center gap-1.5 text-xs text-foreground group">
                <Checkbox
                  checked={isAllSelected}
                  onCheckedChange={() => onSelectAll(isAllSelected ? [] : detectedPrivateRequired.map((a) => a.id))}
                  className="h-3.5 w-3.5"
                  disabled={writeBlocked}
                />
                <span className="transition-opacity group-hover:opacity-80">{t('settings.installPreferences.selectAll')}</span>
              </label>
            </div>
          </div>
        </div>

        {selectableCount > 0 ? (
          <div className="space-y-2">
            {visiblePrivateRequiredAgents.length > 0 ? (
              <div className="flex flex-col gap-1.5">
                {visiblePrivateRequiredAgents.map((agent) => (
                  <SelectableAgentRow
                    key={agent.id}
                    agent={agent}
                    scope={scope}
                    selected={selectedAgentIds.has(agent.id)}
                    onToggle={onToggle}
                    disabled={writeBlocked}
                  />
                ))}
              </div>
            ) : (
              <p className="rounded-md border border-dashed border-border/70 bg-muted/15 px-3 py-2 text-xs leading-5 text-muted-foreground">
                {t('settings.installPreferences.noVisibleAdditionalAgents')}
              </p>
            )}

            {hiddenPrivateRequiredAgents.length > 0 && (
              <Collapsible>
                <CollapsibleTrigger className="group flex w-full items-center justify-center gap-1.5 rounded-md border border-transparent py-1.5 text-xs text-muted-foreground transition-colors hover:bg-muted/40">
                  {t('settings.installPreferences.otherAgentsToggle', { count: hiddenPrivateRequiredAgents.length })}
                  <ChevronDown className="h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180" />
                </CollapsibleTrigger>
                <CollapsibleContent className="mt-1.5 flex flex-col gap-1.5">
                  {hiddenPrivateRequiredAgents.map((agent) => (
                    <SelectableAgentRow
                      key={agent.id}
                      agent={agent}
                      scope={scope}
                      selected={selectedAgentIds.has(agent.id)}
                      onToggle={onToggle}
                      disabled={writeBlocked}
                      muted
                    />
                  ))}
                </CollapsibleContent>
              </Collapsible>
            )}
          </div>
        ) : (
          <p className="rounded-md border border-dashed border-border/70 bg-muted/15 px-3 py-2 text-xs leading-5 text-muted-foreground">
            {t('settings.installPreferences.noAdditionalAgents')}
          </p>
        )}
      </section>
    </div>
  );
}

function SelectableAgentRow({
  agent,
  scope,
  selected,
  onToggle,
  disabled = false,
  muted = false,
}: {
  agent: AgentInfo;
  scope: InstallScope;
  selected: boolean;
  onToggle: (agentId: string) => void;
  disabled?: boolean;
  muted?: boolean;
}) {
  const target = getAgentTarget(agent, scope);

  return (
    <div
      role="button"
      tabIndex={disabled ? -1 : 0}
      aria-disabled={disabled}
      onClick={() => { if (!disabled) onToggle(agent.id); }}
      onKeyDown={(event) => {
        if (!disabled && (event.key === 'Enter' || event.key === ' ')) {
          event.preventDefault();
          onToggle(agent.id);
        }
      }}
      className={cn(
        'group grid w-full cursor-pointer grid-cols-[auto_auto_auto_1fr_auto] items-center gap-3 rounded-md px-3 py-2.5 text-left outline-none transition-all duration-200 focus-visible:ring-2 focus-visible:ring-ring/35 hover:bg-muted/30',
        selected ? 'bg-primary/5' : 'bg-transparent',
        muted && !selected ? 'opacity-80' : 'opacity-100',
        disabled && 'cursor-not-allowed opacity-50'
      )}
    >
      <Checkbox checked={selected} className="pointer-events-none" disabled={disabled} />
      <AgentIcon agentId={agent.id} className="h-7 w-7 rounded-[5px]" />
      <span className={cn('text-[13px] font-medium leading-tight', selected ? 'text-foreground' : 'text-foreground/90')}>
        {agent.name}
      </span>
      <div className="min-w-0 flex justify-end pr-2">
        <code className="truncate font-mono text-[11px] leading-tight text-muted-foreground/50">
          {formatAgentTargetPath(target.path)}
        </code>
      </div>
      <AgentDetectionStatus detected={agent.detected} />
    </div>
  );
}

function AgentDetectionStatus({ detected }: { detected: boolean }) {
  const { t } = useTranslation();

  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-1.5 rounded text-[11px]',
        detected ? 'text-muted-foreground' : 'text-muted-foreground/50'
      )}
    >
      <span
        className={cn(
          'h-1.5 w-1.5 rounded-full',
          detected ? 'bg-emerald-500/80' : 'bg-muted-foreground/30'
        )}
      />
      {t(detected ? 'settings.detected' : 'settings.notDetected')}
    </span>
  );
}

function LoadingRows() {
  return (
    <div className="space-y-4">
      <Skeleton className="h-[200px] w-full rounded-lg" />
      <Skeleton className="h-[300px] w-full rounded-lg" />
    </div>
  );
}
