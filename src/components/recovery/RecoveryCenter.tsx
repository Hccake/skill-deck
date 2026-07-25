import { useEffect } from 'react';
import { AlertTriangle, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { events } from '@/bindings';
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
import { environmentKey } from '@/lib/context';
import { useEnvironmentStore } from '@/stores/environment';
import { useRecoveryStore } from '@/stores/recovery';
import { formatAppError } from '@/utils/format-app-error';

export function RecoveryCenter() {
  const { t } = useTranslation();
  const resources = useRecoveryStore((state) => state.resources);
  const maintenance = useRecoveryStore((state) => state.maintenance ?? []);
  const state = useRecoveryStore((store) => store.state);
  const error = useRecoveryStore((store) => store.error);
  const load = useRecoveryStore((store) => store.load);
  const applyMaintenance = useRecoveryStore((store) => store.applyMaintenance ?? (() => {}));
  const environments = useEnvironmentStore((store) => store.environments);
  const discoveryError = useEnvironmentStore((store) => store.discoveryError);
  const connectionErrors = useEnvironmentStore((store) => store.errorsByEnvironment);
  const environmentRevisionSignature = environments.map((environment) => (
    `${environmentKey(environment.environment)}:${environment.revision}`
  )).join('|');

  useEffect(() => {
    void load();
  }, [environmentRevisionSignature, load]);

  useEffect(() => {
    const refresh = () => void load();
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  }, [load]);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void events.runtimeMaintenanceChanged.listen((event) => {
      if (!disposed) {
        applyMaintenance(event.payload.status);
        void load();
      }
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    }).catch((listenError) => {
      console.error('Failed to listen for runtime maintenance changes:', listenError);
    });
    return () => {
      disposed = true;
      stop?.();
    };
  }, [applyMaintenance, load]);

  const failedMaintenance = maintenance.filter((item) => item.state === 'failed');
  const connectionFailures = Object.entries(connectionErrors).flatMap(([key, itemError]) => {
    if (!itemError) return [];
    const environment = environments.find((item) => environmentKey(item.environment) === key);
    return [{
      key,
      error: itemError,
      environment: environment?.displayName ?? key,
    }];
  });
  const hasIssues = resources.length > 0
    || failedMaintenance.length > 0
    || discoveryError !== null
    || connectionFailures.length > 0
    || error !== null;

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

            {discoveryError ? (
              <section className="border-b pb-4">
                <h3 className="text-sm font-medium">{t('recovery.environment.discoveryTitle')}</h3>
                <p className="mt-1 text-xs text-muted-foreground">
                  {formatAppError(discoveryError, t)}
                </p>
              </section>
            ) : null}

            {connectionFailures.map((failure) => (
              <section key={`connection:${failure.key}`} className="border-b pb-4">
                <h3 className="text-sm font-medium">
                  {t('recovery.environment.connectionTitle', { environment: failure.environment })}
                </h3>
                <p className="mt-1 text-xs text-muted-foreground">
                  {formatAppError(failure.error, t)}
                </p>
              </section>
            ))}

            {failedMaintenance.map((item) => {
              const environment = item.environment.kind === 'host'
                ? t('mutation.host')
                : item.environment.distro_name;
              return (
                <section key={`maintenance:${environmentKey(item.environment)}`} className="border-b pb-4">
                  <h3 className="text-sm font-medium">
                    {t('recovery.maintenance.title', { environment })}
                  </h3>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {t('recovery.maintenance.failed')}
                  </p>
                  {item.issues.length > 0 ? (
                    <ul className="mt-2 list-disc space-y-1 pl-4 text-xs text-muted-foreground">
                      {item.issues.map((issue) => (
                        <li key={issue}>{t(`recovery.maintenance.issues.${issue}`)}</li>
                      ))}
                    </ul>
                  ) : null}
                </section>
              );
            })}

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
