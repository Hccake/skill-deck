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
import type { AgentInfo } from '@/hooks/useTauriApi';

interface ScopeAgentGroups {
  detectedAutomatic: AgentInfo[];
  undetectedAutomatic: AgentInfo[];
  detectedSelectableAgents: AgentInfo[];
  visibleSelectableAgents: AgentInfo[];
  hiddenSelectableAgents: AgentInfo[];
  selectableCount: number;
  isAllSelected: boolean;
}

export function InstallPreferencesPage() {
  const { t } = useTranslation();
  const {
    allAgents,
    agentsLoaded,
    defaultTargetAgents,
    toggleDefaultTargetAgent,
    setDefaultTargetAgents,
  } = useSettingsStore();

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
              agents={allAgents}
              selectedAgents={defaultTargetAgents[scope]}
              loaded={agentsLoaded}
              onToggle={(agentId) => toggleDefaultTargetAgent(scope, agentId)}
              onSelectAll={(agents) => setDefaultTargetAgents(scope, agents)}
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
  onToggle,
  onSelectAll,
}: {
  scope: InstallScope;
  agents: AgentInfo[];
  selectedAgents: string[];
  loaded: boolean;
  onToggle: (agentId: string) => void;
  onSelectAll: (agents: string[]) => void;
}) {
  const { t } = useTranslation();
  const selectedAgentIds = useMemo(() => new Set(selectedAgents), [selectedAgents]);
  const agentGroups = useMemo<ScopeAgentGroups>(() => {
    const groups = groupAgentsByScopedTarget(agents, scope, selectedAgentIds);

    return {
      ...groups,
      isAllSelected: groups.detectedSelectableAgents.length > 0
        && groups.detectedSelectableAgents.every((agent) => selectedAgentIds.has(agent.id)),
    };
  }, [agents, scope, selectedAgentIds]);

  if (!loaded) {
    return <LoadingRows />;
  }

  const {
    detectedAutomatic,
    undetectedAutomatic,
    detectedSelectableAgents,
    visibleSelectableAgents,
    hiddenSelectableAgents,
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
                detected: detectedAutomatic.length,
                undetected: undetectedAutomatic.length,
              })}
            </span>
          </div>
        </div>

        <div className="space-y-3">
          <div className="flex flex-wrap gap-2">
            {detectedAutomatic.map((agent) => (
              <div key={agent.id} className="flex items-center gap-1.5 rounded border border-border/40 bg-muted/10 px-2 py-1">
                <AgentIcon agentId={agent.id} className="h-4 w-4 bg-transparent border-0" iconClassName="h-3.5 w-3.5 text-foreground/80" />
                <span className="text-xs font-medium text-foreground">{agent.name}</span>
              </div>
            ))}
            {detectedAutomatic.length === 0 && (
              <span className="text-xs text-muted-foreground">
                {t('settings.installPreferences.noAutomaticAgents')}
              </span>
            )}
          </div>

          {undetectedAutomatic.length > 0 && (
            <Collapsible>
              <CollapsibleTrigger className="group flex items-center text-xs text-muted-foreground transition-colors hover:text-foreground">
                {t('settings.installPreferences.otherAutomaticAgentsToggle', { count: undetectedAutomatic.length })}
                <ChevronDown className="ml-1 h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180" />
              </CollapsibleTrigger>
              <CollapsibleContent className="mt-2 flex flex-wrap gap-2">
                {undetectedAutomatic.map((agent) => (
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
          <div className="flex items-center justify-between">
            <h3 className="text-[13px] font-semibold tracking-tight text-foreground">
              {t('settings.installPreferences.additionalSection')}
            </h3>
            <div className="flex items-center gap-3">
              <span className="text-xs text-muted-foreground">
                {t('settings.installPreferences.selectedCount', { count: selectedAgents.length })}
              </span>
              <label className="flex cursor-pointer items-center gap-1.5 text-xs text-foreground group">
                <Checkbox
                  checked={isAllSelected}
                  onCheckedChange={() => onSelectAll(isAllSelected ? [] : detectedSelectableAgents.map((a) => a.id))}
                  className="h-3.5 w-3.5"
                />
                <span className="transition-opacity group-hover:opacity-80">{t('settings.installPreferences.selectAll')}</span>
              </label>
            </div>
          </div>
        </div>

        {selectableCount > 0 ? (
          <div className="space-y-2">
            {visibleSelectableAgents.length > 0 ? (
              <div className="flex flex-col gap-1.5">
                {visibleSelectableAgents.map((agent) => (
                  <SelectableAgentRow
                    key={agent.id}
                    agent={agent}
                    scope={scope}
                    selected={selectedAgentIds.has(agent.id)}
                    onToggle={onToggle}
                  />
                ))}
              </div>
            ) : (
              <p className="rounded-md border border-dashed border-border/70 bg-muted/15 px-3 py-2 text-xs leading-5 text-muted-foreground">
                {t('settings.installPreferences.noVisibleAdditionalAgents')}
              </p>
            )}

            {hiddenSelectableAgents.length > 0 && (
              <Collapsible>
                <CollapsibleTrigger className="group flex w-full items-center justify-center gap-1.5 rounded-md border border-transparent py-1.5 text-xs text-muted-foreground transition-colors hover:bg-muted/40">
                  {t('settings.installPreferences.otherAgentsToggle', { count: hiddenSelectableAgents.length })}
                  <ChevronDown className="h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180" />
                </CollapsibleTrigger>
                <CollapsibleContent className="mt-1.5 flex flex-col gap-1.5">
                  {hiddenSelectableAgents.map((agent) => (
                    <SelectableAgentRow
                      key={agent.id}
                      agent={agent}
                      scope={scope}
                      selected={selectedAgentIds.has(agent.id)}
                      onToggle={onToggle}
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
  muted = false,
}: {
  agent: AgentInfo;
  scope: InstallScope;
  selected: boolean;
  onToggle: (agentId: string) => void;
  muted?: boolean;
}) {
  const target = getAgentTarget(agent, scope);

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onToggle(agent.id)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onToggle(agent.id);
        }
      }}
      className={cn(
        'group grid w-full cursor-pointer grid-cols-[auto_auto_auto_1fr_auto] items-center gap-3 rounded-md px-3 py-2.5 text-left outline-none transition-all duration-200 focus-visible:ring-2 focus-visible:ring-ring/35 hover:bg-muted/30',
        selected ? 'bg-primary/5' : 'bg-transparent',
        muted && !selected ? 'opacity-80' : 'opacity-100'
      )}
    >
      <Checkbox checked={selected} className="pointer-events-none" />
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
