import {
  CheckCircle2,
  Circle,
  CircleHelp,
  Copy,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
  Trash2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { AgentIcon } from '@/components/agents/AgentIcon';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type {
  AgentDefinition,
  AgentSource,
  CustomAgentDefinition,
  CustomPathSpec,
  PathSpec,
  ResolvedAgent,
  ResolvedAgentScope,
} from '@/bindings';
import {
  pathRelativeValue,
  scopeReadMode,
  type AgentListItem,
  type ScopeReadMode,
} from './agent-settings-presentation';

interface AgentCardGridProps {
  items: AgentListItem[];
  source: AgentSource;
  query: string;
  actionsDisabled: boolean;
  runtimeState: 'loading' | 'ready' | 'unavailable';
  onClearQuery: () => void;
  onAddCustom: () => void;
  onEdit: (definition: CustomAgentDefinition) => void;
  onDuplicate: (definition: CustomAgentDefinition) => void;
  onDelete: (definition: CustomAgentDefinition) => void;
}

function detectionIcon(state: ResolvedAgent['detection'] | 'loading' | 'unavailable') {
  if (state === 'loading') {
    return (
      <Loader2
        aria-hidden="true"
        className="h-3.5 w-3.5 animate-spin text-muted-foreground motion-reduce:animate-none"
      />
    );
  }
  if (state === 'detected') {
    return (
      <CheckCircle2
        aria-hidden="true"
        className="h-3.5 w-3.5 text-emerald-600 dark:text-emerald-400"
      />
    );
  }
  if (state === 'notDetected') {
    return <Circle aria-hidden="true" className="h-3.5 w-3.5 text-muted-foreground" />;
  }
  return <CircleHelp aria-hidden="true" className="h-3.5 w-3.5 text-warning" />;
}

function privatePathFor(
  scope: 'global' | 'project',
  definition: AgentDefinition,
  customDefinition?: CustomAgentDefinition,
): PathSpec | CustomPathSpec | null {
  return customDefinition?.[scope].privatePath ?? definition[scope].privatePath;
}

function scopeModeFor(
  scope: 'global' | 'project',
  definition: AgentDefinition,
  customDefinition?: CustomAgentDefinition,
): ScopeReadMode {
  return scopeReadMode(customDefinition?.[scope] ?? definition[scope]);
}

function configuredPathLabel(path: PathSpec | CustomPathSpec | null): string | null {
  if (!path) return null;
  if (path.kind === 'absolute') return path.path;
  if (path.kind === 'based') {
    if (path.base === 'home') return `~/${path.relativePath}`;
    return path.relativePath;
  }
  if (path.kind === 'home') return `~/${path.relativePath}`;
  if (path.kind === 'configHome' || path.kind === 'project') return path.relativePath;
  return pathRelativeValue(path);
}

function PathValue({
  value,
  resolvedValue,
  tooltipLabel,
  muted = false,
}: {
  value: string;
  resolvedValue?: string | null;
  tooltipLabel?: string;
  muted?: boolean;
}) {
  const tooltipValue = resolvedValue ?? value;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <code
          tabIndex={0}
          className={cn(
            'inline-block w-fit max-w-full truncate rounded-sm font-mono text-[11px] leading-4 outline-none focus-visible:ring-2 focus-visible:ring-ring',
            muted ? 'text-muted-foreground' : 'text-foreground',
          )}
          translate="no"
        >
          {value}
        </code>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        sideOffset={6}
        showArrow={false}
        className="max-w-[22rem] space-y-1 border bg-popover p-2.5 text-left text-popover-foreground shadow-md text-wrap"
      >
        {tooltipLabel ? (
          <div
            data-slot="agent-path-tooltip-kind"
            className="text-[11px] leading-4 text-muted-foreground"
          >
            {tooltipLabel}
          </div>
        ) : null}
        <div
          data-slot="agent-path-tooltip-value"
          className="font-mono text-xs leading-5 text-foreground [overflow-wrap:anywhere]"
          translate="no"
        >
          {tooltipValue}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

export function SharedDirectoriesReference({
  resolvedGlobalPath,
  runtimeState,
}: {
  resolvedGlobalPath: string | null;
  runtimeState: AgentCardGridProps['runtimeState'];
}) {
  const { t } = useTranslation();
  const globalTooltipValue = runtimeState === 'loading'
    ? t('settings.agents.pathLoading')
    : runtimeState === 'unavailable'
      ? t('settings.agents.pathUnavailable')
      : resolvedGlobalPath ?? t('settings.agents.pathUnavailable');

  return (
    <section
      role="group"
      aria-label={t('settings.agents.sharedDirectories.title')}
      className="flex min-w-0 flex-wrap items-center gap-x-5 gap-y-2 text-xs"
    >
      <h3 className="shrink-0 font-medium text-foreground">
        {t('settings.agents.sharedDirectories.title')}
      </h3>
      <dl className="flex min-w-0 flex-1 flex-wrap items-center gap-x-5 gap-y-2">
        <div className="flex min-w-0 items-center gap-2">
          <dt className="shrink-0 text-muted-foreground">
            {t('settings.agents.global.title')}
          </dt>
          <dd className="min-w-0">
            <PathValue value="~/.agents/skills" resolvedValue={globalTooltipValue} />
          </dd>
        </div>
        <div className="flex min-w-0 items-center gap-2">
          <dt className="shrink-0 text-muted-foreground">
            {t('settings.agents.project.title')}
          </dt>
          <dd className="min-w-0">
            <PathValue value=".agents/skills" />
          </dd>
        </div>
      </dl>
    </section>
  );
}

function AgentPropertyRow({
  label,
  ariaLabel = label,
  children,
  emphasized = false,
}: {
  label: string;
  ariaLabel?: string;
  children: React.ReactNode;
  emphasized?: boolean;
}) {
  return (
    <div
      role="group"
      aria-label={ariaLabel}
      className={cn(
        'grid h-12 min-w-0 grid-cols-[5rem_minmax(0,1fr)] border-t',
        emphasized ? 'border-border/80 bg-muted/10' : 'border-border/45',
      )}
    >
      <div
        data-slot="agent-property-label"
        className={cn(
          'flex items-center whitespace-nowrap border-r px-2.5 text-[11px] font-semibold text-foreground',
          emphasized ? 'border-border/70 bg-muted/30' : 'border-border/45 bg-muted/20',
        )}
      >
        {label}
      </div>
      <div
        data-slot="agent-property-value"
        className="flex min-w-0 flex-col justify-center gap-0.5 px-3"
      >
        {children}
      </div>
    </div>
  );
}

function ScopeDirectories({
  scope,
  definition,
  customDefinition,
  runtime,
  runtimeState,
}: {
  scope: 'global' | 'project';
  definition: AgentDefinition;
  customDefinition?: CustomAgentDefinition;
  runtime?: ResolvedAgentScope;
  runtimeState: AgentCardGridProps['runtimeState'];
}) {
  const { t } = useTranslation();
  const mode = scopeModeFor(scope, definition, customDefinition);
  const privatePath = privatePathFor(scope, definition, customDefinition);
  const privateDisplayPath = configuredPathLabel(privatePath);
  const privateResolvedPath = scope === 'global' ? runtime?.privatePath : null;
  const runtimePlaceholder = runtimeState === 'loading'
    ? t('settings.agents.pathLoading')
    : t('settings.agents.pathUnavailable');
  const scopeLabel = t(`settings.agents.${scope}.title`);
  const ariaLabel = mode === 'unsupported'
    ? t(`settings.agents.readMode.${scope}Unsupported`)
    : t(`settings.agents.sharedDirectories.${mode}AriaLabel`, { scope: scopeLabel });
  const privateValue = privateDisplayPath ? (
    <PathValue
      value={privateDisplayPath}
      resolvedValue={scope === 'global' ? privateResolvedPath ?? runtimePlaceholder : null}
      tooltipLabel={t('settings.agents.directoryKind.private')}
    />
  ) : (
    <span className="text-[11px] leading-5 text-muted-foreground">{runtimePlaceholder}</span>
  );

  return (
    <AgentPropertyRow label={scopeLabel} ariaLabel={ariaLabel}>
      {mode === 'unsupported' ? (
        <p className="text-[11px] leading-4 text-muted-foreground">
          {t(`settings.agents.readMode.${scope}Unsupported`)}
        </p>
      ) : mode === 'shared' ? (
        <span className="w-fit max-w-full truncate text-[11px] leading-4 text-muted-foreground">
          {t('settings.agents.sharedDirectories.cardLabel')}
        </span>
      ) : mode === 'private' ? privateValue : (
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="max-w-[7.5rem] shrink-0 truncate whitespace-nowrap text-[11px] leading-4 text-muted-foreground">
            {t('settings.agents.sharedDirectories.cardLabel')}
          </span>
          <span aria-hidden="true" className="shrink-0 text-[11px] text-muted-foreground">+</span>
          <div className="min-w-0 flex-1">{privateValue}</div>
        </div>
      )}
    </AgentPropertyRow>
  );
}

function detectionPathsFor(item: AgentListItem): Array<PathSpec | CustomPathSpec> {
  if (item.customDefinition) return item.customDefinition.detectionPaths;
  return item.definition.detection.kind === 'anyPathExists'
    ? item.definition.detection.paths
    : [];
}

function detectionPathLabel(path: PathSpec | CustomPathSpec, t: (key: string) => string): string {
  if (path.kind === 'absolute') return path.path;
  if (path.kind === 'environmentVariable') {
    const suffix = path.relativePath ? ` / ${path.relativePath}` : '';
    return `$${path.name}${suffix} -> ${t('settings.agents.pathFallback')} ${detectionPathLabel(path.fallback, t)}`;
  }
  if (path.kind === 'firstExisting') {
    const candidates = path.candidates.map((candidate) => detectionPathLabel(candidate, t));
    return `${candidates.join(' | ')} -> ${t('settings.agents.pathFallback')} ${detectionPathLabel(path.fallback, t)}`;
  }
  const base = path.kind === 'based' ? path.base : path.kind;
  if (base === 'home') return `~/${path.relativePath}`;
  if (base === 'project') return path.relativePath;
  return `${t(`settings.agents.pathLocations.${base}`)} / ${path.relativePath}`;
}

function AgentCard({
  item,
  runtimeState,
  actionsDisabled,
  onEdit,
  onDuplicate,
  onDelete,
}: {
  item: AgentListItem;
  runtimeState: AgentCardGridProps['runtimeState'];
  actionsDisabled: boolean;
  onEdit: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const detection = runtimeState === 'ready'
    ? item.runtime?.detection ?? 'indeterminate'
    : runtimeState;
  const detectionHint = runtimeState === 'ready' && item.runtime?.detectionReason
    ? t(`settings.agents.detectionReasons.${item.runtime.detectionReason}`)
    : t(`settings.agents.preview.detection.${detection}`);
  const detectionPaths = detectionPathsFor(item);
  const detectionLabels = detectionPaths.map((path) => detectionPathLabel(path, t));
  const visibleDetectionLabels = detectionLabels.length <= 2
    ? detectionLabels
    : detectionLabels.slice(0, 1);
  const hiddenDetectionCount = detectionLabels.length - visibleDetectionLabels.length;
  const detectionFallback = item.definition.detection.kind === 'eve'
    ? t('settings.agents.detection.eve')
    : t('settings.agents.pathUnavailable');

  return (
    <article className="grid h-full min-w-0 grid-rows-[3.5rem_repeat(3,3rem)] overflow-hidden rounded-lg border border-border/60 bg-background transition-colors hover:border-border [contain-intrinsic-size:auto_12.5rem] [content-visibility:auto]">
      <header className="grid min-w-0 grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3 px-4">
        <AgentIcon agentId={item.definition.id} className="h-8 w-8 rounded-md" />
        <div className="flex min-w-0 flex-col justify-center">
          <h2 className="truncate text-[13px] font-semibold leading-4 text-foreground" title={item.definition.displayName}>
            {item.definition.displayName}
          </h2>
          <p className="truncate font-mono text-[10px] leading-3.5 text-muted-foreground" translate="no">
            {item.definition.id}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                aria-label={t(`settings.agents.preview.detection.${detection}`)}
                className={cn(
                  'inline-flex size-6 shrink-0 items-center justify-center rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring',
                  detection === 'detected' ? 'text-foreground' : 'text-muted-foreground',
                )}
                tabIndex={0}
              >
                {detectionIcon(detection)}
              </span>
            </TooltipTrigger>
            <TooltipContent side="top" sideOffset={4}>{detectionHint}</TooltipContent>
          </Tooltip>
          {item.customDefinition ? (
            <div className="flex items-center gap-0.5">
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                disabled={actionsDisabled}
                aria-label={t('settings.agents.editNamed', { name: item.definition.displayName })}
                onClick={onEdit}
              >
                <Pencil className="h-3.5 w-3.5" />
              </Button>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    disabled={actionsDisabled}
                    aria-label={t('settings.agents.moreActionsNamed', { name: item.definition.displayName })}
                  >
                    <MoreHorizontal className="h-4 w-4" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem onSelect={onDuplicate}>
                    <Copy className="h-4 w-4" />
                    {t('settings.agents.duplicate')}
                  </DropdownMenuItem>
                  <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={onDelete}>
                    <Trash2 className="h-4 w-4" />
                    {t('settings.agents.delete')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          ) : null}
        </div>
      </header>

      <ScopeDirectories
        scope="global"
        definition={item.definition}
        customDefinition={item.customDefinition}
        runtime={item.runtime?.global}
        runtimeState={runtimeState}
      />
      <ScopeDirectories
        scope="project"
        definition={item.definition}
        customDefinition={item.customDefinition}
        runtime={item.runtime?.project}
        runtimeState={runtimeState}
      />
      <AgentPropertyRow
        label={t('settings.agents.detection.cardLabel')}
        ariaLabel={t('settings.agents.detection.cardTooltip')}
        emphasized
      >
        {(visibleDetectionLabels.length > 0 ? visibleDetectionLabels : [detectionFallback]).map((path) => (
          <div key={path} className="min-w-0">
            <PathValue value={path} muted />
          </div>
        ))}
        {hiddenDetectionCount > 0 ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                className="w-fit shrink-0 rounded-sm text-[10px] leading-4 text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
                tabIndex={0}
                aria-label={t('settings.agents.detection.morePaths', { count: hiddenDetectionCount })}
              >
                +{hiddenDetectionCount}
              </span>
            </TooltipTrigger>
            <TooltipContent
              side="top"
              sideOffset={6}
              showArrow={false}
              className="max-w-[22rem] space-y-1.5 border bg-popover p-2.5 text-left text-popover-foreground shadow-md text-wrap"
            >
              {detectionLabels.map((path) => (
                <div
                  key={path}
                  className="font-mono text-xs leading-5 text-foreground [overflow-wrap:anywhere]"
                  translate="no"
                >
                  {path}
                </div>
              ))}
            </TooltipContent>
          </Tooltip>
        ) : null}
      </AgentPropertyRow>
    </article>
  );
}

export function AgentCardGrid({
  items,
  source,
  query,
  actionsDisabled,
  runtimeState,
  onClearQuery,
  onAddCustom,
  onEdit,
  onDuplicate,
  onDelete,
}: AgentCardGridProps) {
  const { t } = useTranslation();
  const hasQuery = query.trim().length > 0;

  if (items.length === 0) {
    return (
      <div className="flex min-h-52 flex-col items-center justify-center rounded-lg border border-dashed border-border/60 px-6 py-10 text-center">
        <p className="text-sm font-medium text-foreground">
          {t(hasQuery
            ? 'settings.agents.empty.searchTitle'
            : source === 'custom'
              ? 'settings.agents.empty.customTitle'
              : 'settings.agents.empty.builtinTitle', { query })}
        </p>
        <p className="mt-1 max-w-sm text-xs leading-5 text-muted-foreground">
          {t(hasQuery
            ? 'settings.agents.empty.searchDescription'
            : source === 'custom'
              ? 'settings.agents.empty.customDescription'
              : 'settings.agents.empty.builtinDescription')}
        </p>
        {hasQuery ? (
          <Button type="button" variant="link" size="sm" onClick={onClearQuery}>
            {t('settings.agents.empty.clearSearch')}
          </Button>
        ) : source === 'custom' ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="mt-3"
            disabled={actionsDisabled}
            onClick={onAddCustom}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('settings.agents.add')}
          </Button>
        ) : null}
      </div>
    );
  }

  return (
    <div
      role="list"
      aria-label={t('settings.agents.listLabel')}
      className="grid grid-cols-[repeat(auto-fill,minmax(min(100%,18rem),1fr))] items-stretch gap-3"
    >
      {items.map((item) => (
        <div key={item.definition.id} role="listitem" className="h-full min-w-0">
          <AgentCard
            item={item}
            actionsDisabled={actionsDisabled}
            runtimeState={runtimeState}
            onEdit={() => item.customDefinition && onEdit(item.customDefinition)}
            onDuplicate={() => item.customDefinition && onDuplicate(item.customDefinition)}
            onDelete={() => item.customDefinition && onDelete(item.customDefinition)}
          />
        </div>
      ))}
    </div>
  );
}
