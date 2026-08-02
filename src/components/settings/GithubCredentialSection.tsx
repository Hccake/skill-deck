import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CircleHelp, Github, KeyRound, Loader2, RefreshCw, ShieldCheck, Trash2 } from 'lucide-react';
import type {
  GithubCredentialStatus,
  GithubCredentialValidationStatus,
} from '@/bindings';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useSettingsStore } from '@/stores/settings';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

function statusKey(validation: GithubCredentialValidationStatus): string {
  return `settings.githubCredential.status.${validation}`;
}

function sourceKey(status: GithubCredentialStatus): string {
  return `settings.githubCredential.source.${status.source}`;
}

function GithubMark({ large = false }: { large?: boolean }) {
  return (
    <div
      className={`${large ? 'size-9' : 'size-8'} flex shrink-0 items-center justify-center rounded-md bg-muted/70 text-foreground ring-1 ring-inset ring-border/60`}
      aria-hidden="true"
    >
      <Github className={large ? 'size-[18px]' : 'size-4'} />
    </div>
  );
}

interface CredentialFeedback {
  kind: GithubCredentialValidationStatus | 'saved' | 'clearFailed' | 'suppressionCleanupFailed';
  retryAtEpochMs?: number | null;
}

type CredentialViewMode = 'pending' | 'configured' | 'unconfigured' | 'unavailable';

function isValidationFeedback(
  kind: CredentialFeedback['kind'],
): kind is GithubCredentialValidationStatus {
  return ['unconfigured', 'verified', 'invalid', 'rateLimited', 'unavailable'].includes(kind);
}

function isInvalidTokenFeedback(kind: CredentialFeedback['kind']): boolean {
  return kind === 'unconfigured' || kind === 'invalid';
}

function getCredentialViewMode(status: GithubCredentialStatus | null): CredentialViewMode {
  if (!status) return 'pending';
  if (status.storage === 'unavailable') return 'unavailable';
  if (status.source === 'keyring') return 'configured';
  return 'unconfigured';
}

export function GithubCredentialSection() {
  const { t, i18n } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const status = useSettingsStore((state) => state.githubCredential.status);
  const loadState = useSettingsStore((state) => state.githubCredential.loadState);
  const saving = useSettingsStore((state) => state.githubCredential.saving);
  const clearing = useSettingsStore((state) => state.githubCredential.clearing);
  const error = useSettingsStore((state) => state.githubCredential.error);
  const loadCredential = useSettingsStore((state) => state.loadGithubCredential);
  const saveCredential = useSettingsStore((state) => state.saveGithubCredential);
  const clearCredential = useSettingsStore((state) => state.clearGithubCredential);
  const [token, setToken] = useState('');
  const [feedback, setFeedback] = useState<CredentialFeedback | null>(null);
  const [configureOpen, setConfigureOpen] = useState(false);
  const [removeOpen, setRemoveOpen] = useState(false);

  useEffect(() => {
    void loadCredential();
  }, [loadCredential]);

  const busy = saving || clearing;

  function openConfigureDialog() {
    if (writeBlocked) return;
    setToken('');
    setFeedback(null);
    setConfigureOpen(true);
  }

  function handleConfigureOpenChange(open: boolean) {
    if (!open && saving) return;
    setConfigureOpen(open);
    if (!open) {
      setToken('');
      if (feedback && isValidationFeedback(feedback.kind)) setFeedback(null);
    }
  }

  async function handleSave() {
    if (writeBlocked || !token.trim()) return;
    setFeedback(null);
    const result = await saveCredential(token);
    if (!result) return;
    if (result.saved) {
      setToken('');
      setConfigureOpen(false);
      setFeedback({
        kind: result.warnings.includes('suppressionCleanupFailed')
          ? 'suppressionCleanupFailed'
          : 'saved',
      });
      return;
    }
    if (result.status.storage === 'unavailable') {
      setToken('');
      setConfigureOpen(false);
      setFeedback(null);
      return;
    }
    setFeedback({
      kind: result.status.validation,
      retryAtEpochMs: result.status.retryAtEpochMs,
    });
  }

  const viewMode = getCredentialViewMode(status);

  async function handleClear() {
    if (writeBlocked) return;
    setFeedback(null);
    const result = await clearCredential();
    if (!result) return;
    if (!result.cleared) {
      setFeedback({ kind: 'clearFailed' });
      return;
    }
    setRemoveOpen(false);
    if (result.warnings.includes('suppressionCleanupFailed')) {
      setFeedback({ kind: 'suppressionCleanupFailed' });
    }
  }

  const statusSummary = (() => {
    if (!status) return t('settings.githubCredential.description');
    if (status.storage === 'unavailable') {
      if (status.source === 'githubTokenEnv' || status.source === 'ghTokenEnv') {
        return t('settings.githubCredential.storageUnavailableWithEnvironment', {
          source: t(sourceKey(status)),
        });
      }
      return t('settings.githubCredential.storageUnavailable');
    }
    if (status.source === 'none') return t('settings.githubCredential.description');

    const parts = [status.account, t(sourceKey(status))].filter(Boolean);
    if (status.rateLimitRemaining !== null && status.rateLimitLimit !== null) {
      const formatter = new Intl.NumberFormat(i18n.language);
      parts.push(
        `${formatter.format(status.rateLimitRemaining)} / ${formatter.format(status.rateLimitLimit)}`,
      );
    }
    if (status.validation !== 'verified') {
      parts.push(t(statusKey(status.validation)));
    }
    return parts.join(' · ');
  })();

  if (loadState === 'loading' && !status) {
    return (
      <section className="px-4 py-4 sm:px-5" aria-label={t('settings.githubCredential.title')}>
        <div className="flex items-center gap-2.5">
          <Skeleton className="size-8 rounded-md" />
          <div className="space-y-1.5">
            <Skeleton className="h-4 w-32" />
            <Skeleton className="h-3 w-56" />
          </div>
        </div>
      </section>
    );
  }

  return (
    <section className="px-4 py-4 sm:px-5" aria-labelledby="github-credential-title">
      <div className="flex min-w-0 items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3">
          <GithubMark />
          <div className="min-w-0">
            <div className="flex items-center gap-1.5">
              <h2 id="github-credential-title" className="text-sm font-medium text-foreground">
                {t('settings.githubCredential.title')}
              </h2>
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    className="inline-flex size-5 shrink-0 touch-manipulation items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1"
                    aria-label={t('settings.githubCredential.environmentHelpLabel')}
                  >
                    <CircleHelp className="size-3.5" aria-hidden="true" />
                  </button>
                </TooltipTrigger>
                <TooltipContent className="max-w-72 text-xs leading-5">
                  {t('settings.githubCredential.environmentHelp')}
                </TooltipContent>
              </Tooltip>
            </div>
            <p className="mt-1 truncate text-xs text-muted-foreground">
              {statusSummary}
            </p>
          </div>
        </div>
        {loadState === 'error' || viewMode === 'unavailable' ? (
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="w-24"
            disabled={busy || loadState === 'loading'}
            onClick={() => void loadCredential()}
          >
            <RefreshCw
              className={loadState === 'loading' ? 'animate-spin motion-reduce:animate-none' : undefined}
              aria-hidden="true"
            />
            <span aria-live="polite">
              {t(loadState === 'loading'
                ? 'settings.githubCredential.checking'
                : 'settings.githubCredential.recheck')}
            </span>
          </Button>
        ) : viewMode === 'configured' ? (
          <div className="flex shrink-0 items-center gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || writeBlocked}
              onClick={openConfigureDialog}
            >
              <KeyRound aria-hidden="true" />
              {t('settings.githubCredential.replace')}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="border-destructive/30 text-destructive hover:border-destructive/50 hover:bg-destructive/10 hover:text-destructive"
              disabled={busy || writeBlocked}
              onClick={() => setRemoveOpen(true)}
            >
              <Trash2 aria-hidden="true" />
              {t('settings.githubCredential.remove')}
            </Button>
          </div>
        ) : viewMode === 'unconfigured' ? (
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={writeBlocked}
            onClick={openConfigureDialog}
          >
            {t('settings.githubCredential.configure')}
          </Button>
        ) : null}
      </div>

      <Dialog open={configureOpen} onOpenChange={handleConfigureOpenChange}>
        <DialogContent
          className="gap-0 overflow-hidden overscroll-contain p-0 sm:max-w-[30rem]"
          closeLabel={t('common.close')}
        >
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void handleSave();
            }}
          >
            <div className="p-5 sm:p-6">
              <DialogHeader className="pr-8">
                <div className="flex items-start gap-3 text-left">
                  <GithubMark large />
                  <div className="min-w-0 space-y-1">
                    <DialogTitle className="text-pretty text-base leading-6">
                      {t(viewMode === 'configured'
                        ? 'settings.githubCredential.replaceTitle'
                        : 'settings.githubCredential.configureTitle')}
                    </DialogTitle>
                    <DialogDescription className="text-pretty text-xs leading-5">
                      {t('settings.githubCredential.dialogDescription')}
                    </DialogDescription>
                  </div>
                </div>
              </DialogHeader>
              <div className="mt-6 space-y-2">
                <Label htmlFor="github-token-dialog" className="sr-only">
                  {t('settings.githubCredential.tokenLabel')}
                </Label>
                <Input
                  id="github-token-dialog"
                  name="github-token"
                  type="password"
                  autoComplete="off"
                  autoCapitalize="none"
                  spellCheck={false}
                  translate="no"
                  aria-describedby={feedback && isValidationFeedback(feedback.kind)
                    ? 'github-token-dialog-help github-token-dialog-feedback'
                    : 'github-token-dialog-help'}
                  aria-invalid={feedback ? isInvalidTokenFeedback(feedback.kind) : undefined}
                  value={token}
                  placeholder={t('settings.githubCredential.tokenPlaceholder')}
                  className="h-10 font-mono text-sm"
                  disabled={busy || writeBlocked}
                  onChange={(event) => {
                    setToken(event.target.value);
                    setFeedback(null);
                  }}
                />
                <p id="github-token-dialog-help" className="text-xs text-muted-foreground">
                  {t('settings.githubCredential.savedTokenHidden')}
                </p>
                {feedback && isValidationFeedback(feedback.kind) ? (
                  <div id="github-token-dialog-feedback" role="status" className="space-y-1 text-xs text-destructive">
                    <p>{t(`settings.githubCredential.feedback.${feedback.kind}`)}</p>
                    {feedback.kind === 'rateLimited' && feedback.retryAtEpochMs ? (
                      <p>
                        {t('settings.githubCredential.feedback.retryAt', {
                          time: new Date(feedback.retryAtEpochMs).toLocaleString(i18n.language),
                        })}
                      </p>
                    ) : null}
                  </div>
                ) : null}
                {error && configureOpen ? (
                  <p role="alert" className="text-xs text-destructive">
                    {t('settings.githubCredential.requestFailed')}
                  </p>
                ) : null}
              </div>
            </div>
            <DialogFooter className="border-t bg-muted/20 px-5 py-3.5 sm:px-6">
              <DialogClose asChild>
                <Button type="button" size="sm" variant="outline">
                  {t('common.cancel')}
                </Button>
              </DialogClose>
              <Button type="submit" size="sm" disabled={writeBlocked || busy || !token.trim()}>
                {saving
                  ? <Loader2 className="animate-spin motion-reduce:animate-none" aria-hidden="true" />
                  : <ShieldCheck aria-hidden="true" />}
                {t('settings.githubCredential.verifyAndSave')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={removeOpen}
        onOpenChange={(open) => {
          if (!open && clearing) return;
          setRemoveOpen(open);
          if (!open && feedback?.kind === 'clearFailed') setFeedback(null);
        }}
      >
        <AlertDialogContent className="gap-0 overflow-hidden overscroll-contain p-0 sm:max-w-md">
          <div className="p-5 sm:p-6">
            <AlertDialogHeader className="flex flex-row items-start gap-3 text-left">
              <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-destructive/10 text-destructive ring-1 ring-inset ring-destructive/15">
                <Trash2 className="size-[18px]" aria-hidden="true" />
              </div>
              <div className="min-w-0 space-y-1">
                <AlertDialogTitle className="text-pretty text-base leading-6">
                  {t('settings.githubCredential.removeTitle')}
                </AlertDialogTitle>
                <AlertDialogDescription className="text-pretty text-xs leading-5">
                  {t('settings.githubCredential.removeDescription')}
                </AlertDialogDescription>
                {feedback?.kind === 'clearFailed' ? (
                  <p role="alert" className="pt-1 text-xs text-destructive">
                    {t('settings.githubCredential.feedback.clearFailed')}
                  </p>
                ) : null}
                {error && removeOpen ? (
                  <p role="alert" className="pt-1 text-xs text-destructive">
                    {t('settings.githubCredential.requestFailed')}
                  </p>
                ) : null}
              </div>
            </AlertDialogHeader>
          </div>
          <AlertDialogFooter className="border-t bg-muted/20 px-5 py-3.5 sm:px-6">
            <AlertDialogCancel size="sm">{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              size="sm"
              variant="destructive"
              disabled={writeBlocked || clearing}
              onClick={(event) => {
                event.preventDefault();
                void handleClear();
              }}
            >
              {clearing
                ? <Loader2 className="animate-spin motion-reduce:animate-none" aria-hidden="true" />
                : <Trash2 aria-hidden="true" />}
              {t('settings.githubCredential.confirmRemove')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {feedback && (feedback.kind === 'saved' || feedback.kind === 'suppressionCleanupFailed') ? (
        <div
          role="status"
          className="mt-3 text-xs sm:pl-11"
        >
          <p className={feedback.kind === 'saved' ? 'text-success' : 'text-warning'}>
            {t(`settings.githubCredential.feedback.${feedback.kind}`)}
          </p>
        </div>
      ) : null}
      {error && !configureOpen && !removeOpen ? (
        <p role="alert" className="mt-3 text-xs text-destructive sm:pl-11">
          {t('settings.githubCredential.requestFailed')}
        </p>
      ) : null}
    </section>
  );
}
