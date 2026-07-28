import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Github, Loader2, RefreshCw, ShieldCheck, Trash2 } from 'lucide-react';
import type {
  GithubCredentialStatus,
  GithubCredentialValidationStatus,
} from '@/bindings';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Skeleton } from '@/components/ui/skeleton';
import { useSettingsStore } from '@/stores/settings';

function statusKey(validation: GithubCredentialValidationStatus): string {
  return `settings.githubCredential.status.${validation}`;
}

function sourceKey(status: GithubCredentialStatus): string {
  return `settings.githubCredential.source.${status.source}`;
}

interface CredentialFeedback {
  kind: GithubCredentialValidationStatus | 'saved' | 'clearFailed' | 'suppressionCleanupFailed';
  retryAtEpochMs?: number | null;
}

export function GithubCredentialSection() {
  const { t, i18n } = useTranslation();
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

  useEffect(() => {
    void loadCredential();
  }, [loadCredential]);

  const busy = saving || clearing;

  async function handleSave() {
    if (!token.trim()) return;
    setFeedback(null);
    const result = await saveCredential(token);
    if (!result) return;
    if (result.saved) {
      setToken('');
      setFeedback({
        kind: result.warnings.includes('suppressionCleanupFailed')
          ? 'suppressionCleanupFailed'
          : 'saved',
      });
      return;
    }
    setFeedback({
      kind: result.status.validation,
      retryAtEpochMs: result.status.retryAtEpochMs,
    });
  }

  async function handleClear() {
    setFeedback(null);
    const result = await clearCredential();
    if (!result) return;
    if (!result.cleared) {
      setFeedback({ kind: 'clearFailed' });
    } else if (result.warnings.includes('suppressionCleanupFailed')) {
      setFeedback({ kind: 'suppressionCleanupFailed' });
    }
  }

  if (loadState === 'loading' && !status) {
    return (
      <section className="space-y-4" aria-label={t('settings.githubCredential.title')}>
        <div className="flex items-center gap-2.5">
          <Skeleton className="h-7 w-7 rounded-md" />
          <div className="space-y-1.5">
            <Skeleton className="h-4 w-32" />
            <Skeleton className="h-3 w-56" />
          </div>
        </div>
        <Skeleton className="h-9 w-full" />
      </section>
    );
  }

  return (
    <section className="space-y-4" aria-labelledby="github-credential-title">
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 items-start gap-2.5">
          <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-accent">
            <Github className="h-4 w-4 text-accent-foreground" />
          </div>
          <div className="min-w-0">
            <h2 id="github-credential-title" className="text-sm font-heading font-bold text-foreground">
              {t('settings.githubCredential.title')}
            </h2>
            <p className="text-xs text-muted-foreground">
              {t('settings.githubCredential.description')}
            </p>
          </div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          title={t('settings.githubCredential.refresh')}
          aria-label={t('settings.githubCredential.refresh')}
          disabled={busy || loadState === 'loading'}
          onClick={() => void loadCredential()}
        >
          <RefreshCw className={loadState === 'loading' ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
        </Button>
      </div>

      {status ? (
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <Badge variant={status.validation === 'verified' ? 'default' : 'secondary'}>
            {t(statusKey(status.validation))}
          </Badge>
          <span>{t(sourceKey(status))}</span>
          {status.account ? (
            <span className="inline-flex items-center gap-1 font-medium text-foreground">
              <ShieldCheck className="h-3.5 w-3.5" />
              {status.account}
            </span>
          ) : null}
          {status.rateLimitRemaining !== null && status.rateLimitLimit !== null ? (
            <span>
              {t('settings.githubCredential.rateLimit', {
                remaining: status.rateLimitRemaining,
                limit: status.rateLimitLimit,
              })}
            </span>
          ) : null}
        </div>
      ) : null}

      {status?.storage === 'unavailable' ? (
        <Alert>
          <AlertDescription>
            {t('settings.githubCredential.storageUnavailable')} {' '}
            {t('settings.githubCredential.environmentFallback')}
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="space-y-2">
        <Label htmlFor="github-token">{t('settings.githubCredential.tokenLabel')}</Label>
        <Input
          id="github-token"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={token}
          placeholder={t('settings.githubCredential.tokenPlaceholder')}
          disabled={busy}
          onChange={(event) => {
            setToken(event.target.value);
            setFeedback(null);
          }}
        />
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            size="sm"
            disabled={busy || !token.trim() || status?.storage === 'unavailable'}
            onClick={() => void handleSave()}
          >
            {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <ShieldCheck className="h-4 w-4" />}
            {t('settings.githubCredential.verifyAndSave')}
          </Button>
          {status?.source === 'keyring' ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => void handleClear()}
            >
              {clearing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
              {t('settings.githubCredential.clear')}
            </Button>
          ) : null}
        </div>
      </div>

      {feedback ? (
        <div
          role="status"
          className={feedback.kind === 'saved'
            ? 'space-y-1 text-xs text-success'
            : feedback.kind === 'suppressionCleanupFailed'
              ? 'space-y-1 text-xs text-warning'
              : 'space-y-1 text-xs text-destructive'}
        >
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
      {error ? (
        <p role="alert" className="text-xs text-destructive">
          {t('settings.githubCredential.requestFailed')}
        </p>
      ) : null}
    </section>
  );
}
