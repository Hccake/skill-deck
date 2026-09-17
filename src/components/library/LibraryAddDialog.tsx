import { useEffect, useMemo, useRef } from 'react';
import {
  AlertCircle,
  ChevronDown,
  CheckCircle2,
  LoaderCircle,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { SkillSearch } from '@/components/skills/skill-search/SkillSearch';
import { SourceSkillSelectionPanel } from '@/components/source-discovery/SourceSkillSelectionPanel';
import { RedirectHostConfirmation } from '@/components/source-discovery/RedirectHostConfirmation';
import { formatAppError } from '@/utils/format-app-error';
import { cn } from '@/lib/utils';
import { contextKey, sameContext } from '@/lib/context';
import { displayPath } from '@/lib/display-path';
import { useProjectWorkspace } from '@/hooks/useProjectWorkspace';
import type { EnvironmentRef, LibraryMembershipPreview } from '@/bindings';
import {
  useLibraryAddFlow,
  type ExecuteLibraryCommand,
  type LibraryAddFlow,
  type LibraryAddPhase,
  type LibraryAddTarget,
} from './useLibraryAddFlow';
import { MembershipImpactSummary } from './MembershipImpactSummary';
import { MembershipOutcomeSummary } from './MembershipOutcomeSummary';
import { LibraryUsageIdentity } from './LibraryUsageIdentity';

interface LibraryAddDialogProps {
  open: boolean;
  target: LibraryAddTarget;
  existingSkillNames: ReadonlySet<string>;
  execute: ExecuteLibraryCommand;
  onClose: () => void;
}

const STEP_PHASES: Record<'source' | 'selection' | 'review', LibraryAddPhase[]> = {
  source: ['source'],
  selection: ['selection'],
  review: ['preparing', 'review', 'executing', 'result'],
};
const EMPTY_SKILL_KEYS = new Set<string>();

export function LibraryAddDialog({
  open,
  target,
  existingSkillNames,
  execute,
  onClose,
}: LibraryAddDialogProps) {
  const { t } = useTranslation();
  const flow = useLibraryAddFlow({ target, existingSkillNames, execute, onClose });
  const locked = flow.phase === 'preparing' || flow.phase === 'executing';
  const bodyRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const dialog = bodyRef.current?.closest('[role="dialog"]');
    const step = dialog?.querySelector<HTMLElement>('[aria-current="step"]');
    (step ?? bodyRef.current)?.focus({ preventScroll: true });
  }, [flow.phase]);

  return (
    <TooltipProvider>
      <Dialog open={open} onOpenChange={(nextOpen) => {
        if (!nextOpen) void flow.close();
      }}>
        <DialogContent
          className="library-add-dialog grid h-[min(680px,calc(100vh-24px))] w-[min(920px,calc(100vw-24px))] max-w-none grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0 sm:max-w-none"
          dismissible={!locked}
          showCloseButton={!locked}
          closeLabel={t('common.close')}
          aria-busy={locked}
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            const dialog = bodyRef.current?.closest('[role="dialog"]');
            const step = dialog?.querySelector<HTMLElement>('[aria-current="step"]');
            (step ?? bodyRef.current)?.focus({ preventScroll: true });
          }}
        >
          <LibraryAddHeader target={target} phase={flow.phase} />
          <div
            ref={bodyRef}
            tabIndex={-1}
            className="min-h-0 overflow-hidden px-5 py-4 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50 sm:px-6"
          >
            <LibraryAddBody flow={flow} environment={target.environment} />
          </div>
          <LibraryAddFooter flow={flow} />
        </DialogContent>
      </Dialog>
    </TooltipProvider>
  );
}

function LibraryAddHeader({
  target,
  phase,
}: {
  target: LibraryAddTarget;
  phase: LibraryAddPhase;
}) {
  const { t } = useTranslation();
  const activeStep = phase === 'result'
    ? null
    : STEP_PHASES.source.includes(phase)
      ? 'source'
      : STEP_PHASES.selection.includes(phase)
        ? 'selection'
        : 'review';
  const steps = ['source', 'selection', 'review'] as const;
  const activeIndex = activeStep === null ? steps.length : steps.indexOf(activeStep);
  return (
    <DialogHeader className="library-add-header gap-3 border-b px-5 pb-4 pt-5 pr-14 text-left sm:px-6 sm:pr-14">
      <div className="flex min-w-0 items-center gap-3">
        <DialogTitle className="min-w-0 flex-1 truncate text-base" translate="no">
          {t('libraries.addFlow.title', { library: target.libraryName })}
        </DialogTitle>
        <DialogDescription
          className="max-w-[40%] shrink-0 truncate text-xs"
          title={target.environmentName}
          translate="no"
        >
          {target.environmentName}
        </DialogDescription>
      </div>
      <ol className="grid grid-cols-3 gap-2" aria-label={t('libraries.addFlow.steps.label')}>
        {steps.map((step, index) => {
          const state = index < activeIndex ? 'complete' : index === activeIndex ? 'current' : 'upcoming';
          return (
            <li
              key={step}
              className={cn(
                'flex min-w-0 items-center gap-2 border-t-2 pt-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/50',
                state === 'current' && 'border-primary font-medium text-foreground',
                state === 'complete' && 'border-primary/45 text-muted-foreground',
                state === 'upcoming' && 'border-border text-muted-foreground',
              )}
              aria-current={state === 'current' ? 'step' : undefined}
              tabIndex={state === 'current' ? -1 : undefined}
            >
              <span className="tabular-nums">{index + 1}</span>
              <span className="truncate">{t(`libraries.addFlow.steps.${step}`)}</span>
            </li>
          );
        })}
      </ol>
    </DialogHeader>
  );
}

function LibraryAddBody({ flow, environment }: { flow: LibraryAddFlow; environment: EnvironmentRef }) {
  if (flow.phase === 'source') return <LibrarySourceStep flow={flow} />;
  if (flow.phase === 'selection') return <LibrarySkillSelectionStep flow={flow} />;
  if (flow.phase === 'preparing') {
    return <LibraryAddProgress labelKey="libraries.addFlow.preparing" />;
  }
  if (flow.phase === 'executing') {
    return <LibraryAddProgress labelKey="libraries.addFlow.executing" />;
  }
  if (flow.phase === 'result') return <LibraryAddResultStep flow={flow} />;
  return <LibraryAddReviewStep flow={flow} environment={environment} />;
}

function LibrarySourceStep({ flow }: { flow: LibraryAddFlow }) {
  const { t } = useTranslation();
  const composingRef = useRef(false);
  const isLoading = flow.discovery.status === 'loading';
  const progress = flow.discovery.cloneProgress;
  const loadingText = progress?.phase === 'connecting'
    ? t('addSkill.source.status.connecting')
    : progress?.phase === 'cloning'
      ? t('addSkill.source.status.cloningWithTime', {
        elapsed: progress.elapsed_secs,
        timeout: progress.timeout_secs,
      })
      : t('libraries.addFlow.source.loading');

  return (
    <Tabs defaultValue="manual" className="flex h-full min-h-0 flex-col">
      <TabsList className="mb-4 grid w-64 shrink-0 grid-cols-2">
        <TabsTrigger value="search" disabled={isLoading}>
          {t('addSkill.source.tabs.search')}
        </TabsTrigger>
        <TabsTrigger value="manual" disabled={isLoading}>
          {t('addSkill.source.tabs.manual')}
        </TabsTrigger>
      </TabsList>

      {isLoading ? (
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3" role="status" aria-live="polite">
          <LoaderCircle className="size-6 animate-spin text-primary" aria-hidden="true" />
          <p className="text-sm font-medium">{loadingText}</p>
          <p className="max-w-full truncate font-mono text-xs text-muted-foreground" translate="no">
            {flow.sourceInput}
          </p>
        </div>
      ) : (
        <>
          <TabsContent value="search" className="min-h-0 flex-1 overflow-hidden">
            <SkillSearch
              installedSkillKeys={EMPTY_SKILL_KEYS}
              onInstall={(skill) => void flow.selectSearchResult(skill)}
              actionLabel={t('libraries.addFlow.source.add')}
            />
          </TabsContent>
          <TabsContent value="manual" className="min-h-0 flex-1 overflow-auto overscroll-contain">
            <div className="max-w-2xl space-y-3">
              <Label htmlFor="library-add-source">{t('libraries.addFlow.source.label')}</Label>
              <div className="flex min-w-0 gap-2">
                <Input
                  id="library-add-source"
                  name="library-add-source"
                  value={flow.sourceInput}
                  onChange={(event) => flow.setSourceInput(event.target.value)}
                  onCompositionStart={() => {
                    composingRef.current = true;
                  }}
                  onCompositionEnd={() => {
                    composingRef.current = false;
                  }}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' && !composingRef.current) void flow.readSource();
                  }}
                  placeholder={t('libraries.sourcePlaceholder')}
                  autoComplete="off"
                  spellCheck={false}
                  className="min-w-0 flex-1"
                  translate="no"
                />
                <Button
                  type="button"
                  onClick={() => void flow.readSource()}
                  disabled={!flow.sourceInput.trim()}
                  className="shrink-0"
                >
                  {t('libraries.addFlow.source.read')}
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">{t('addSkill.source.hint')}</p>
            </div>
          </TabsContent>
        </>
      )}

      {flow.discovery.status === 'error' && flow.discovery.error ? (
        <div role="alert" className="mt-3 flex shrink-0 items-start justify-between gap-3 border-t pt-3 text-sm text-destructive">
          <span className="min-w-0 break-words">{formatAppError(flow.discovery.error, t)}</span>
          <Button type="button" variant="outline" size="sm" onClick={() => void flow.retryDiscovery()}>
            {t('common.retry')}
          </Button>
        </div>
      ) : null}
    </Tabs>
  );
}

function LibrarySkillSelectionStep({ flow }: { flow: LibraryAddFlow }) {
  const { t } = useTranslation();
  const candidates = useMemo(() => flow.candidates.map((candidate) => ({
    ...candidate,
    statusLabel: candidate.statusLabel === 'alreadyInLibrary'
      ? t('libraries.addFlow.selection.alreadyInLibrary')
      : candidate.statusLabel,
  })), [flow.candidates, t]);

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {flow.agentIntentIgnored ? (
        <p className="shrink-0 text-xs text-muted-foreground" role="status">
          {t('libraries.addFlow.selection.agentIntentIgnored')}
        </p>
      ) : null}
      {flow.selectableCount === 0 ? (
        <div className="flex flex-1 items-center justify-center text-center text-sm text-muted-foreground">
          {t('libraries.addFlow.selection.allExisting')}
        </div>
      ) : (
        <SourceSkillSelectionPanel
          candidates={candidates}
          selectedCandidateIds={flow.selectedCandidateIds}
          query={flow.selectionQuery}
          onQueryChange={flow.setSelectionQuery}
          onSelectionChange={flow.setSelectedCandidateIds}
          copy={{
            title: t('libraries.addFlow.selection.title'),
            selected: (count, total) => t('libraries.addFlow.selection.count', { selected: count, available: total }),
            searchPlaceholder: t('addSkill.skills.search'),
            selectAll: t('libraries.addFlow.selection.selectAll'),
            clear: t('addSkill.skills.clear'),
            empty: t('addSkill.skills.empty'),
            generalGroup: t('skills.pluginGroup.general'),
          }}
        />
      )}
      {flow.flowError ? (
        <p role="alert" className="shrink-0 text-sm text-destructive">
          {formatAppError(flow.flowError, t)}
        </p>
      ) : null}
      {flow.flowIssue ? (
        <p role="alert" className="shrink-0 text-sm text-destructive">
          {t(`libraries.addFlow.error.${flow.flowIssue}`)}
        </p>
      ) : null}
    </div>
  );
}

function LibraryAddReviewStep({ flow, environment }: { flow: LibraryAddFlow; environment: EnvironmentRef }) {
  const { t } = useTranslation();
  const preview = flow.prepared?.preview;
  const unverifiedCount = preview?.membership.impacts.reduce(
    (count, impact) => count + impact.skills.filter((skill) => skill.kind === 'unverified').length,
    0,
  ) ?? 0;
  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <p className="text-sm font-medium">
          {t('libraries.addFlow.review.summary', { count: preview?.skills.length ?? 0 })}
        </p>
        {preview && preview.membership.scopes.length > 0 ? (
          <Popover>
            <PopoverTrigger asChild>
              <button type="button" className="inline-flex items-center gap-1 rounded-sm text-xs text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
                {t('libraries.addFlow.review.syncCount', { count: preview.membership.scopes.length })}
                <ChevronDown className="size-3.5" aria-hidden="true" />
              </button>
            </PopoverTrigger>
            <PopoverContent align="end" className="max-h-[min(320px,50vh)] w-80 max-w-[calc(100vw-48px)] overflow-y-auto overscroll-contain">
              <LibraryAddSyncDetails membership={preview.membership} />
            </PopoverContent>
          </Popover>
        ) : null}
      </div>
      {preview && !preview.membership.inventoryComplete ? (
        <p role="alert" className="shrink-0 text-xs text-warning">
          {t('libraries.membership.inventoryIncomplete', { count: preview.membership.scopes.length })}
        </p>
      ) : null}
      {unverifiedCount > 0 ? (
        <p role="alert" className="shrink-0 text-xs text-warning">
          {t('libraries.membership.impact.unverified', { count: unverifiedCount })}
        </p>
      ) : null}
      <ul className="min-h-0 flex-1 overflow-y-auto overscroll-contain rounded-md border">
        {preview?.skills.map((skill) => (
          <li key={skill.skillName} className="flex min-w-0 items-baseline gap-3 border-b px-3 py-2.5 text-sm last:border-b-0">
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="min-w-0 max-w-[40%] shrink-0 truncate rounded-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring/50" tabIndex={0} translate="no">
                  {skill.skillName}
                </span>
              </TooltipTrigger>
              <TooltipContent className="max-w-[min(560px,calc(100vw-32px))] break-all" translate="no">{skill.skillName}</TooltipContent>
            </Tooltip>
            <LibraryAddPath path={skill.targetPath} environment={environment} />
          </li>
        ))}
      </ul>
      {preview?.redirectedDownloadHost ? (
        <RedirectHostConfirmation
          host={preview.redirectedDownloadHost}
          acknowledged={flow.redirectAcknowledged}
          onAcknowledgedChange={flow.setRedirectAcknowledged}
        />
      ) : null}
      {flow.flowError ? (
        <p role="alert" className="shrink-0 text-sm text-destructive">
          {formatAppError(flow.flowError, t)}
        </p>
      ) : null}
      {flow.flowIssue ? (
        <p role="alert" className="shrink-0 text-sm text-destructive">
          {t(`libraries.addFlow.error.${flow.flowIssue}`)}
        </p>
      ) : null}
    </div>
  );
}

function LibraryAddPath({ path, environment }: { path: string; environment: EnvironmentRef }) {
  const fullPath = displayPath(path, environment);
  const separator = environment.kind === 'wsl' || fullPath.startsWith('/') ? '/' : '\\';
  const split = fullPath.lastIndexOf(separator) + 1;
  return (
    <div className="min-w-0 flex-1">
      <Tooltip>
        <TooltipTrigger asChild>
          <code tabIndex={0} translate="no" className="flex w-fit min-w-0 max-w-full rounded-sm text-left text-xs text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
            <span className="min-w-0 truncate">{fullPath.slice(0, split)}</span>
            <span className="max-w-[65%] shrink-0 truncate">{fullPath.slice(split)}</span>
          </code>
        </TooltipTrigger>
        <TooltipContent className="max-w-[min(560px,calc(100vw-32px))] break-all font-mono text-xs" translate="no">
          {fullPath}
        </TooltipContent>
      </Tooltip>
    </div>
  );
}

function LibraryAddSyncDetails({ membership }: { membership: LibraryMembershipPreview }) {
  const { projects } = useProjectWorkspace(membership.environment);
  return (
    <ul className="divide-y divide-border/60">
      {membership.scopes.map((context) => {
        const scope = context.scope;
        const project = scope.scope === 'project'
          ? projects.find((item) => item.binding.id === scope.project_id)?.binding ?? null
          : null;
        const impact = membership.impacts.find((item) => sameContext(item.context, context));
        return (
          <li key={contextKey(context)} className="space-y-1.5 py-2 first:pt-0 last:pb-0">
            <LibraryUsageIdentity usage={{ context, project, state: 'confirmed' }} showPath={false} />
            {impact ? <MembershipImpactSummary preview={{ ...membership, impacts: [impact] }} /> : null}
          </li>
        );
      })}
    </ul>
  );
}

function LibraryAddProgress({ labelKey }: { labelKey: string }) {
  const { t } = useTranslation();
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3" role="status" aria-live="polite">
      <LoaderCircle className="size-7 animate-spin text-primary" aria-hidden="true" />
      <p className="text-sm font-medium">{t(labelKey)}</p>
    </div>
  );
}

function LibraryAddResultStep({ flow }: { flow: LibraryAddFlow }) {
  const { t } = useTranslation();
  const succeeded = flow.results.filter((result) => result.status === 'succeeded').length;
  const failed = flow.results.length - succeeded;

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <div className="flex shrink-0 items-center gap-3" role="status" aria-live="polite">
        {failed === 0 ? (
          <CheckCircle2 className="size-5 text-success" aria-hidden="true" />
        ) : (
          <AlertCircle className="size-5 text-warning" aria-hidden="true" />
        )}
        <p className="text-sm font-semibold">
          {failed === 0
            ? t('libraries.addFlow.result.succeeded', { count: succeeded })
            : t('libraries.addFlow.result.partial', { succeeded, failed })}
        </p>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain rounded-md border">
        {flow.results.map((result) => (
          <div key={result.skillName} className="border-b px-3 py-2.5 last:border-b-0">
            <div className="flex min-w-0 items-center justify-between gap-3 text-sm">
              <span className="min-w-0 break-words font-medium [overflow-wrap:anywhere]" translate="no">
                {result.skillName}
              </span>
              <span className={result.status === 'succeeded' ? 'text-success' : 'text-destructive'}>
                {t(`libraries.addResult.${result.status}`)}
              </span>
            </div>
            {result.error ? (
              <p className="mt-1 break-words text-xs text-destructive" role="alert">
                {formatAppError(result.error, t)}
              </p>
            ) : null}
          </div>
        ))}
      </div>
      {flow.membershipOutcome ? (
        <MembershipOutcomeSummary outcome={flow.membershipOutcome} />
      ) : null}
      {flow.flowError ? (
        <p role="alert" className="shrink-0 text-sm text-destructive">
          {formatAppError(flow.flowError, t)}
        </p>
      ) : null}
      {flow.flowIssue ? (
        <p role="alert" className="shrink-0 text-sm text-destructive">
          {t(`libraries.addFlow.error.${flow.flowIssue}`)}
        </p>
      ) : null}
    </div>
  );
}

function LibraryAddFooter({ flow }: { flow: LibraryAddFlow }) {
  const { t } = useTranslation();
  const canBack = flow.phase === 'selection' || flow.phase === 'review';
  const locked = flow.phase === 'preparing' || flow.phase === 'executing';
  const hasRetry = flow.phase === 'result' && flow.prepared !== null;
  const completed = flow.phase === 'result' && !hasRetry
    && flow.results.every((result) => result.status === 'succeeded');

  return (
    <DialogFooter className="min-h-16 flex-row items-center justify-end border-t px-5 py-3 sm:justify-end sm:px-6">
      {!locked ? (
        <Button type="button" variant={completed ? 'default' : 'ghost'} onClick={() => void flow.close()}>
          {completed ? t('libraries.addFlow.result.done') : flow.phase === 'result' ? t('common.close') : t('common.cancel')}
        </Button>
      ) : null}
      {canBack ? (
        <Button type="button" variant="outline" onClick={() => void flow.back()}>
          {t('addSkill.actions.back')}
        </Button>
      ) : null}
      {flow.phase === 'selection' ? (
        <Button
          type="button"
          className="min-w-28"
          onClick={() => void flow.prepare()}
          disabled={flow.selectedCandidateIds.length === 0}
        >
          {t('libraries.addFlow.selection.review')}
        </Button>
      ) : null}
      {flow.phase === 'review' ? (
        <Button
          type="button"
          className="min-w-28"
          onClick={() => void flow.executePrepared()}
          disabled={Boolean(
            flow.prepared?.preview.redirectedDownloadHost
            && !flow.redirectAcknowledged
          )}
        >
          {t('libraries.addFlow.review.confirm')}
        </Button>
      ) : null}
      {locked ? (
        <Button type="button" disabled className="min-w-28">
          <LoaderCircle className="size-4 animate-spin" aria-hidden="true" />
          {t(flow.phase === 'executing' ? 'libraries.addFlow.review.adding' : 'libraries.addFlow.preparing')}
        </Button>
      ) : null}
      {hasRetry ? (
        <Button type="button" onClick={() => void flow.retryFailed()}>
          {t('libraries.addFlow.result.retry')}
        </Button>
      ) : null}
    </DialogFooter>
  );
}
