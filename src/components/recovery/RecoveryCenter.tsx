import { useEffect } from 'react';
import { AlertTriangle, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import { useRecoveryStore } from '@/stores/recovery';
import { formatAppError } from '@/utils/format-app-error';

export function RecoveryCenter() {
  const { t } = useTranslation();
  const resources = useRecoveryStore((state) => state.resources);
  const state = useRecoveryStore((store) => store.state);
  const error = useRecoveryStore((store) => store.error);
  const load = useRecoveryStore((store) => store.load);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const refresh = () => void load();
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  }, [load]);

  const hasIssues = resources.length > 0 || error !== null;

  if (!hasIssues) return null;

  return (
    <Dialog>
      <DialogTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="relative h-8 w-8 shrink-0 text-warning hover:text-warning sm:h-9 sm:w-9"
          aria-label={t('recovery.center.open')}
          title={t('recovery.center.open')}
        >
          <AlertTriangle className="h-4 w-4 sm:h-5 sm:w-5" />
          <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-destructive" aria-hidden="true" />
        </Button>
      </DialogTrigger>
      <DialogContent
        className="max-h-[min(80vh,42rem)] overflow-hidden p-0 sm:max-w-xl"
        closeLabel={t('common.close')}
      >
        <DialogHeader className="border-b px-5 pb-4 pt-5 pr-12">
          <DialogTitle>{t('recovery.center.title')}</DialogTitle>
          <DialogDescription>{t('recovery.center.description')}</DialogDescription>
        </DialogHeader>

        <div className="overflow-y-auto px-5 pb-5">
          <div className="flex justify-end py-3">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={state === 'loading'}
              onClick={() => void load()}
            >
              <RefreshCw className={state === 'loading' ? 'animate-spin' : undefined} />
              {t('recovery.refresh')}
            </Button>
          </div>

          <div className="space-y-4">
            {error ? (
              <section className="border-b pb-4">
                <h3 className="text-sm font-medium">{t('recovery.center.loadError')}</h3>
                <p className="mt-1 text-xs text-muted-foreground">{formatAppError(error, t)}</p>
              </section>
            ) : null}

            {resources.map((resource) => (
              <RecoveryActions
                key={resource.resourceId}
                recovery={{
                  resourceId: resource.resourceId,
                  suggestedActionCode: 'openRecoveryResource',
                }}
                initialStatus={resource}
                onResolved={() => void load()}
              />
            ))}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
