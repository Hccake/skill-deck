import { useEffect, useState } from 'react';
import { AlertTriangle, ChevronDown, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { RecoveryActions } from '@/components/recovery/RecoveryActions';
import { Button } from '@/components/ui/button';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from '@/components/ui/collapsible';
import { useEnvironmentStore } from '@/stores/environment';
import { useRecoveryStore } from '@/stores/recovery';
import { events } from '@/bindings';
import { environmentKey } from '@/lib/context';

export function RecoveryCenter() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const resources = useRecoveryStore((state) => state.resources);
  const maintenance = useRecoveryStore((state) => state.maintenance ?? []);
  const state = useRecoveryStore((store) => store.state);
  const error = useRecoveryStore((store) => store.error);
  const load = useRecoveryStore((store) => store.load);
  const applyMaintenance = useRecoveryStore((store) => store.applyMaintenance ?? (() => {}));
  const retryMaintenance = useRecoveryStore((store) => store.retryMaintenance ?? (async () => {}));
  const environmentRevisionSignature = useEnvironmentStore((store) => (
    store.environments.map((environment) => (
      `${environment.environment.kind === 'host' ? 'host' : environment.environment.distro_name}:${environment.revision}`
    )).join('|')
  ));

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
    }).catch(() => {});
    return () => {
      disposed = true;
      stop?.();
    };
  }, [applyMaintenance, load]);

  const pendingMaintenance = maintenance.filter((item) => item.state !== 'ready');
  if (resources.length === 0 && pendingMaintenance.length === 0 && !error) return null;

  return (
    <Collapsible open={open} onOpenChange={setOpen} className="border-b border-warning/30 bg-warning/5 px-4 py-2">
      <div className="mx-auto flex w-full max-w-6xl items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2 text-sm">
          <AlertTriangle className="h-4 w-4 shrink-0 text-warning" />
          <span className="font-medium">
            {error
              ? t('recovery.center.loadError')
              : pendingMaintenance.length > 0
                ? t('recovery.center.maintenanceCount', { count: pendingMaintenance.length })
                : t('recovery.center.count', { count: resources.length })}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button type="button" variant="ghost" size="sm" disabled={state === 'loading'} onClick={() => void load()}>
            <RefreshCw className="h-3.5 w-3.5" />{t('recovery.refresh')}
          </Button>
          {resources.length > 0 || pendingMaintenance.length > 0 ? (
            <CollapsibleTrigger asChild>
              <Button type="button" variant="ghost" size="sm">
                {t(open ? 'recovery.center.hide' : 'recovery.center.show')}
                <ChevronDown className={`h-3.5 w-3.5 transition-transform ${open ? 'rotate-180' : ''}`} />
              </Button>
            </CollapsibleTrigger>
          ) : null}
        </div>
      </div>
      <CollapsibleContent className="mx-auto w-full max-w-6xl space-y-3 pb-2 pt-1">
        {pendingMaintenance.map((item) => {
          const environment = item.environment.kind === 'host'
            ? t('mutation.host')
            : item.environment.distro_name;
          return (
            <div key={`maintenance:${environmentKey(item.environment)}`} className="rounded-md border border-warning/30 bg-background/70 p-3">
              <p className="text-sm font-medium">{t('recovery.maintenance.title', { environment })}</p>
              <p className="mt-1 text-xs text-muted-foreground">
                {t(`recovery.maintenance.${item.state}`)}
              </p>
              {item.state === 'failed' ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="mt-2"
                  onClick={() => void retryMaintenance(item.environment).catch(() => load())}
                >
                  <RefreshCw className="h-3.5 w-3.5" />{t('recovery.retryMaintenance')}
                </Button>
              ) : null}
            </div>
          );
        })}
        {resources.map((resource) => (
          resource.state === 'environmentUnavailable' ? (
            <div key={resource.resourceId} className="rounded-md border border-warning/30 bg-background/70 p-3">
              <p className="text-sm font-medium">{t('recovery.title')}</p>
              <p className="mt-1 text-xs text-muted-foreground">
                {t('recovery.state.environmentUnavailable')}
              </p>
              <Button type="button" variant="outline" size="sm" className="mt-2" onClick={() => void load()}>
                <RefreshCw className="h-3.5 w-3.5" />{t('recovery.refresh')}
              </Button>
            </div>
          ) : (
            <RecoveryActions
              key={resource.resourceId}
              recovery={{
                resourceId: resource.resourceId,
                suggestedActionCode: 'openRecoveryResource',
              }}
              initialStatus={resource}
              onResolved={() => void load()}
            />
          )
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
}
