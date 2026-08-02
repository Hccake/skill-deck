import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { AgentIcon } from '@/components/agents/AgentIcon';
import { cn } from '@/lib/utils';
import {
  formatAgentTargetPath,
  getAgentDisplayPath,
  getSharedSkillDirectory,
  groupAgentsByScopedTarget,
  type InstallScope,
} from '@/lib/agentTargets';
import { useSettingsStore } from '@/stores/settings';
import { agentDisplayName, agentId, isAgentDetected } from '@/lib/agents';
import {
  buildAgentSelectionRows,
  isSelectionRowSelected,
  toggleSelectionRow,
  type AgentSelectionRow,
} from '@/lib/agentSelection';
import type { AgentId, AgentSelectionGroup, DetectionState, EnvironmentRef, ResolvedAgent } from '@/bindings';
import type { AgentDefaultsSnapshot } from '@/stores/settings';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

interface ScopeAgentGroups {
  detectedDefaultAvailable: ResolvedAgent[];
  undetectedDefaultAvailable: ResolvedAgent[];
  notDetectedDefaultAvailable: ResolvedAgent[];
  indeterminateDefaultAvailable: ResolvedAgent[];
  visibleDefaultAvailableAgents: ResolvedAgent[];
  hiddenDefaultAvailableAgents: ResolvedAgent[];
  detectedPrivateRequired: ResolvedAgent[];
  visiblePrivateRequiredAgents: ResolvedAgent[];
  hiddenPrivateRequiredAgents: ResolvedAgent[];
  selectableCount: number;
}

export function InstallPreferencesPage({
  environment,
  snapshot,
}: {
  environment: EnvironmentRef;
  snapshot: AgentDefaultsSnapshot;
}) {
  const { t } = useTranslation();
  const businessWriteBlocked = useBusinessWriteBlocked();
  const saveAgentDefaults = useSettingsStore((state) => state.saveAgentDefaults);
  const loadAgentDefaults = useSettingsStore((state) => state.loadAgentDefaults);
  const writeBlocked = businessWriteBlocked || snapshot.loadState !== 'ready' || snapshot.saving;
  const loaded = snapshot.agents.length > 0
    || snapshot.loadState === 'ready'
    || snapshot.loadState === 'error'
    || snapshot.loadState === 'stale';

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

      {snapshot.error ? (
        <div
          role="alert"
          className="flex max-w-3xl items-center justify-between gap-3 rounded-md border border-destructive/30 bg-destructive/5 p-3"
        >
          <p className="text-sm text-destructive">
            {t(snapshot.loadState === 'stale'
              ? 'settings.installPreferences.staleSave'
              : 'settings.installPreferences.loadError')}
          </p>
          <Button type="button" variant="outline" size="sm" onClick={() => void loadAgentDefaults(environment)}>
            {t('common.retry')}
          </Button>
        </div>
      ) : null}

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
              selectionGroups={snapshot.selectionGroups[scope]}
              selectedAgents={snapshot.defaults[scope]}
              loaded={loaded}
              writeBlocked={writeBlocked}
              onToggle={(agentIds) => {
                const selected = snapshot.defaults[scope];
                saveScope(scope, toggleSelectionRow(selected, agentIds));
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
  selectionGroups,
  selectedAgents,
  loaded,
  writeBlocked,
  onToggle,
  onSelectAll,
}: {
  scope: InstallScope;
  agents: ResolvedAgent[];
  selectionGroups: AgentSelectionGroup[];
  selectedAgents: string[];
  loaded: boolean;
  writeBlocked: boolean;
  onToggle: (agentIds: AgentId[]) => void;
  onSelectAll: (agents: string[]) => void;
}) {
  const { t } = useTranslation();
  const selectedAgentIds = useMemo(() => new Set(selectedAgents), [selectedAgents]);
  const agentGroups = useMemo<ScopeAgentGroups>(() => {
    const groups = groupAgentsByScopedTarget(agents, scope, selectedAgentIds);

    return groups;
  }, [agents, scope, selectedAgentIds]);
  const selectionRows = useMemo(
    () => buildAgentSelectionRows(
      agents,
      selectionGroups,
      [
        ...agentGroups.visiblePrivateRequiredAgents,
        ...agentGroups.hiddenPrivateRequiredAgents,
      ],
    ),
    [agentGroups.hiddenPrivateRequiredAgents, agentGroups.visiblePrivateRequiredAgents, agents, selectionGroups],
  );
  const visibleSelectionRows = useMemo(
    () => selectionRows.filter((row) => isSelectionRowSelected(row, selectedAgentIds)
      || row.agents.some((agent) => agent.definition.source === 'custom' || isAgentDetected(agent))),
    [selectedAgentIds, selectionRows],
  );
  const visibleGroupIds = useMemo(
    () => new Set(visibleSelectionRows.map((row) => row.groupId)),
    [visibleSelectionRows],
  );
  const hiddenSelectionRows = useMemo(
    () => selectionRows.filter((row) => !visibleGroupIds.has(row.groupId)),
    [selectionRows, visibleGroupIds],
  );
  const detectedSelectionRows = useMemo(
    () => selectionRows.filter((row) => row.agents.some(isAgentDetected)),
    [selectionRows],
  );

  if (!loaded) {
    return <LoadingRows />;
  }

  const {
    detectedDefaultAvailable,
    notDetectedDefaultAvailable,
    indeterminateDefaultAvailable,
    visibleDefaultAvailableAgents,
    hiddenDefaultAvailableAgents,
    selectableCount,
  } = agentGroups;
  const isAllSelected = detectedSelectionRows.length > 0
    && detectedSelectionRows.every((row) => isSelectionRowSelected(row, selectedAgentIds));

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
                undetected: notDetectedDefaultAvailable.length,
                indeterminate: indeterminateDefaultAvailable.length,
              })}
            </span>
          </div>
        </div>

        <div className="space-y-3">
          <div className="flex flex-wrap gap-2">
            {visibleDefaultAvailableAgents.map((agent) => (
              <div
                key={agentId(agent)}
                className={cn(
                  'flex items-center gap-1.5 rounded border border-border/40 bg-muted/10 px-2 py-1',
                  !isAgentDetected(agent) && 'opacity-70',
                )}
              >
                <AgentIcon agentId={agentId(agent)} className="h-4 w-4 bg-transparent border-0" iconClassName="h-3.5 w-3.5 text-foreground/80" />
                <span className="text-xs font-medium text-foreground">{agentDisplayName(agent)}</span>
              </div>
            ))}
            {visibleDefaultAvailableAgents.length === 0 && (
              <span className="text-xs text-muted-foreground">
                {t('settings.installPreferences.noAutomaticAgents')}
              </span>
            )}
          </div>

          {hiddenDefaultAvailableAgents.length > 0 && (
            <Collapsible>
              <CollapsibleTrigger className="group flex items-center text-xs text-muted-foreground transition-colors hover:text-foreground">
                {t('settings.installPreferences.otherAutomaticAgentsToggle', { count: hiddenDefaultAvailableAgents.length })}
                <ChevronDown className="ml-1 h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180" />
              </CollapsibleTrigger>
              <CollapsibleContent className="mt-2 flex flex-wrap gap-2">
                {hiddenDefaultAvailableAgents.map((agent) => (
                  <div key={agentId(agent)} className="flex items-center gap-1.5 rounded border border-border/40 bg-muted/5 px-2 py-1 opacity-70">
                    <AgentIcon agentId={agentId(agent)} className="h-4 w-4 bg-transparent border-0" iconClassName="h-3.5 w-3.5 text-muted-foreground/80" />
                    <span className="text-xs font-medium text-muted-foreground">{agentDisplayName(agent)}</span>
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
                  onCheckedChange={() => onSelectAll(
                    isAllSelected
                      ? []
                      : [...new Set(detectedSelectionRows.flatMap((row) => row.selectableAgentIds))],
                  )}
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
            {visibleSelectionRows.length > 0 ? (
              <div className="flex flex-col gap-1.5">
                {visibleSelectionRows.map((row) => (
                  <SelectableAgentRow
                    key={row.groupId}
                    row={row}
                    scope={scope}
                    selected={isSelectionRowSelected(row, selectedAgentIds)}
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

            {hiddenSelectionRows.length > 0 && (
              <Collapsible>
                <CollapsibleTrigger className="group flex w-full items-center justify-center gap-1.5 rounded-md border border-transparent py-1.5 text-xs text-muted-foreground transition-colors hover:bg-muted/40">
                  {t('settings.installPreferences.otherAgentsToggle', { count: hiddenSelectionRows.length })}
                  <ChevronDown className="h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180" />
                </CollapsibleTrigger>
                <CollapsibleContent className="mt-1.5 flex flex-col gap-1.5">
                  {hiddenSelectionRows.map((row) => (
                    <SelectableAgentRow
                      key={row.groupId}
                      row={row}
                      scope={scope}
                      selected={isSelectionRowSelected(row, selectedAgentIds)}
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
  row,
  scope,
  selected,
  onToggle,
  disabled = false,
  muted = false,
}: {
  row: AgentSelectionRow;
  scope: InstallScope;
  selected: boolean;
  onToggle: (agentIds: AgentId[]) => void;
  disabled?: boolean;
  muted?: boolean;
}) {
  const singleAgent = row.agents.length === 1 ? row.agents[0] : null;
  const checkboxId = `install-preference-${scope}-${row.groupId.replace(/[^a-zA-Z0-9_-]/g, '-')}`;
  const displayPath = singleAgent ? getAgentDisplayPath(singleAgent, scope) : null;

  return (
    <Label
      htmlFor={checkboxId}
      className={cn(
        'group grid w-full cursor-pointer grid-cols-[auto_auto_auto_1fr_auto] items-center gap-3 rounded-md px-3 py-2.5 text-left outline-none transition-all duration-200 focus-visible:ring-2 focus-visible:ring-ring/35 hover:bg-muted/30',
        selected ? 'bg-primary/5' : 'bg-transparent',
        muted && !selected ? 'opacity-80' : 'opacity-100',
        disabled && 'cursor-not-allowed opacity-50'
      )}
    >
      <Checkbox
        id={checkboxId}
        checked={selected}
        onCheckedChange={() => onToggle(row.selectableAgentIds)}
        disabled={disabled}
      />
      <span className="flex shrink-0 items-center -space-x-1">
        {row.agents.slice(0, 3).map((agent) => (
          <AgentIcon key={agentId(agent)} agentId={agentId(agent)} className="h-7 w-7 rounded-[5px] ring-1 ring-background" />
        ))}
      </span>
      <span className={cn('text-[13px] font-medium leading-tight', selected ? 'text-foreground' : 'text-foreground/90')}>
        {row.agents.map(agentDisplayName).join(' / ')}
      </span>
      <div className="min-w-0 flex justify-end pr-2">
        <code className="truncate font-mono text-[11px] leading-tight text-muted-foreground/50">
          {displayPath ? formatAgentTargetPath(displayPath) : ''}
        </code>
      </div>
      {singleAgent ? <AgentDetectionStatus detection={singleAgent.detection} /> : <span />}
    </Label>
  );
}

function AgentDetectionStatus({ detection }: { detection: DetectionState }) {
  const { t } = useTranslation();

  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-1.5 rounded text-[11px]',
        detectionTextClass(detection),
      )}
    >
      <span
        className={cn(
          'h-1.5 w-1.5 rounded-full',
          detectionDotClass(detection),
        )}
      />
      {t(`settings.agents.preview.detection.${detection}`)}
    </span>
  );
}

function detectionTextClass(detection: DetectionState): string {
  if (detection === 'detected') return 'text-muted-foreground';
  if (detection === 'indeterminate') return 'text-amber-700/80 dark:text-amber-300/80';
  return 'text-muted-foreground/50';
}

function detectionDotClass(detection: DetectionState): string {
  if (detection === 'detected') return 'bg-emerald-500/80';
  if (detection === 'indeterminate') return 'bg-amber-500/70';
  return 'bg-muted-foreground/30';
}

function LoadingRows() {
  return (
    <div className="space-y-4">
      <Skeleton className="h-[200px] w-full rounded-lg" />
      <Skeleton className="h-[300px] w-full rounded-lg" />
    </div>
  );
}
