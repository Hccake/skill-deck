import { useEffect, useMemo, useRef } from 'react';
import {
  AlertCircle,
  ArrowLeft,
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
import {
  useLibraryAddFlow,
  type ExecuteLibraryCommand,
  type LibraryAddFlow,
  type LibraryAddPhase,
  type LibraryAddTarget,
} from './useLibraryAddFlow';

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
            <LibraryAddBody flow={flow} />
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

function LibraryAddBody({ flow }: { flow: LibraryAddFlow }) {
  if (flow.phase === 'source') return <LibrarySourceStep flow={flow} />;
  if (flow.phase === 'selection') return <LibrarySkillSelectionStep flow={flow} />;
  if (flow.phase === 'preparing') {
    return <LibraryAddProgress labelKey="libraries.addFlow.preparing" />;
  }
  if (flow.phase === 'executing') {
    return <LibraryAddProgress labelKey="libraries.addFlow.executing" />;
  }
  if (flow.phase === 'result') return <LibraryAddResultStep flow={flow} />;
  return <LibraryAddReviewStep flow={flow} />;
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

function LibraryAddReviewStep({ flow }: { flow: LibraryAddFlow }) {
  const { t } = useTranslation();
  const preview = flow.prepared?.preview;
  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <p className="shrink-0 text-sm font-medium">
        {t('libraries.addFlow.review.summary', { count: preview?.skills.length ?? 0 })}
      </p>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain rounded-md border">
        {preview?.skills.map((skill) => (
          <div key={skill.skillName} className="grid min-w-0 grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-4 border-b px-3 py-2.5 text-sm last:border-b-0">
            <span className="min-w-0 break-words font-medium [overflow-wrap:anywhere]" translate="no">
              {skill.skillName}
            </span>
            <Tooltip>
              <TooltipTrigger asChild>
                <code className="min-w-0 truncate text-right text-xs text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50" tabIndex={0} translate="no">
                  {skill.targetPath}
                </code>
              </TooltipTrigger>
              <TooltipContent className="max-w-[min(560px,calc(100vw-32px))] break-all font-mono text-xs">
                {skill.targetPath}
              </TooltipContent>
            </Tooltip>
          </div>
        ))}
      </div>
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

  return (
    <DialogFooter className="flex-row items-center justify-end border-t px-5 py-3 sm:justify-end sm:px-6">
      {!locked ? (
        <Button type="button" variant="outline" onClick={() => void flow.close()}>
          {flow.phase === 'result' ? t('common.close') : t('common.cancel')}
        </Button>
      ) : null}
      {canBack ? (
        <Button type="button" variant="ghost" onClick={() => void flow.back()}>
          <ArrowLeft className="size-4" aria-hidden="true" />
          {t('addSkill.actions.back')}
        </Button>
      ) : null}
      {flow.phase === 'selection' ? (
        <Button
          type="button"
          onClick={() => void flow.prepare()}
          disabled={flow.selectedCandidateIds.length === 0}
        >
          {t('libraries.addFlow.selection.review')}
        </Button>
      ) : null}
      {flow.phase === 'review' ? (
        <Button
          type="button"
          onClick={() => void flow.executePrepared()}
          disabled={Boolean(
            flow.prepared?.preview.redirectedDownloadHost
            && !flow.redirectAcknowledged
          )}
        >
          {t('libraries.addFlow.review.confirm')}
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
