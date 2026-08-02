import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { LoaderCircle } from 'lucide-react';
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
import { Switch } from '@/components/ui/switch';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { useEnvironmentStore } from '@/stores/environment';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { formatAppError } from '@/utils/format-app-error';

export function WslIntegrationSection() {
  const { t } = useTranslation();
  const supported = useEnvironmentStore((state) => state.wslIntegrationSupported);
  const enabled = useEnvironmentStore((state) => state.wslIntegrationEnabled);
  const selectedEnvironment = useWorkspaceContextStore(
    (state) => state.selectedContext.environment,
  );
  const transition = useWorkspaceContextStore((state) => state.transition);
  const changeWslIntegration = useWorkspaceContextStore((state) => state.changeWslIntegration);
  const failure = useWorkspaceContextStore((state) => state.wslIntegrationFailure);
  const clearFailure = useWorkspaceContextStore((state) => state.clearWslIntegrationFailure);
  const writeBlocked = useBusinessWriteBlocked();
  const [confirmOpen, setConfirmOpen] = useState(false);

  if (!supported) return null;

  const changeSetting = async (nextEnabled: boolean) => {
    const outcome = await changeWslIntegration(nextEnabled);
    if (outcome.status === 'succeeded') {
      setConfirmOpen(false);
    }
  };

  const saving = transition.kind === 'wslIntegration';
  const disabled = transition.kind !== 'idle' || writeBlocked;
  const errorMessage = failure ? formatAppError(failure.error, t) : null;
  const hostAlreadySelected = selectedEnvironment.kind === 'host';
  const activeDisableLabel = transition.kind === 'wslIntegration'
    ? transition.phase === 'switchingHost'
      ? 'settings.general.wslSwitchingHost'
      : transition.phase === 'disabling'
        ? 'settings.general.wslDisabling'
        : null
    : null;

  return (
    <>
      <section className="grid gap-4 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
        <div className="min-w-0 space-y-1">
          <p className="text-sm font-medium text-foreground">
            {t('settings.general.wslTitle')}
          </p>
          <p id="wsl-integration-description" className="text-xs leading-5 text-muted-foreground">
            {t('settings.general.wslDescription')}
          </p>
          {errorMessage && !confirmOpen ? (
            <p role="alert" className="text-xs leading-5 text-destructive">
              {errorMessage}
            </p>
          ) : null}
        </div>

        <div className="flex h-8 min-w-14 items-center justify-end gap-2">
          {saving ? (
            <LoaderCircle
              role="status"
              aria-label={t('settings.general.wslSaving')}
              className="h-4 w-4 animate-spin text-muted-foreground motion-reduce:animate-none"
            />
          ) : (
            <span aria-hidden="true" className="h-4 w-4" />
          )}
          <Switch
            checked={enabled}
            disabled={disabled}
            aria-label={t('settings.general.wslTitle')}
            aria-describedby="wsl-integration-description"
            onCheckedChange={(nextEnabled) => {
              if (!nextEnabled && selectedEnvironment.kind === 'wsl') {
                clearFailure();
                setConfirmOpen(true);
                return;
              }
              void changeSetting(nextEnabled);
            }}
          />
        </div>
      </section>

      <AlertDialog
        open={confirmOpen}
        onOpenChange={(open) => {
          if (!saving) {
            setConfirmOpen(open);
            if (!open) clearFailure();
          }
        }}
      >
        <AlertDialogContent dismissible={!saving} aria-busy={saving}>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.general.wslDisableTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t(hostAlreadySelected
                ? 'settings.general.wslDisableAfterHostDescription'
                : 'settings.general.wslDisableDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {errorMessage ? (
            <p role="alert" className="text-sm leading-5 text-destructive">
              {errorMessage}
            </p>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={saving}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              disabled={saving}
              onClick={(event) => {
                event.preventDefault();
                void changeSetting(false);
              }}
            >
              {activeDisableLabel ? (
                <>
                  <LoaderCircle
                    role="status"
                    aria-label={t(activeDisableLabel)}
                    className="h-4 w-4 animate-spin motion-reduce:animate-none"
                  />
                  {t(activeDisableLabel)}
                </>
              ) : t(hostAlreadySelected
                ? 'settings.general.wslDisableOnlyConfirm'
                : 'settings.general.wslDisableConfirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
