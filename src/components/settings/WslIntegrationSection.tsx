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

const HOST = { kind: 'host' } as const;

export function WslIntegrationSection() {
  const { t } = useTranslation();
  const supported = useEnvironmentStore((state) => state.wslIntegrationSupported);
  const enabled = useEnvironmentStore((state) => state.wslIntegrationEnabled);
  const setEnabled = useEnvironmentStore((state) => state.setWslIntegrationEnabled);
  const selectedEnvironment = useWorkspaceContextStore(
    (state) => state.selectedContext.environment,
  );
  const pendingEnvironment = useWorkspaceContextStore((state) => state.pendingEnvironment);
  const switchEnvironment = useWorkspaceContextStore((state) => state.switchEnvironment);
  const writeBlocked = useBusinessWriteBlocked();
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  if (!supported) return null;

  const changeSetting = async (nextEnabled: boolean) => {
    setSaving(true);
    setSaveError(false);
    try {
      if (!nextEnabled && selectedEnvironment.kind === 'wsl') {
        await switchEnvironment(HOST);
      }
      await setEnabled(nextEnabled);
      setConfirmOpen(false);
    } catch (error) {
      console.error('Failed to update WSL integration setting:', error);
      setSaveError(true);
    } finally {
      setSaving(false);
    }
  };

  const disabled = saving || writeBlocked || pendingEnvironment !== null;

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
          {saveError ? (
            <p role="alert" className="text-xs leading-5 text-destructive">
              {t('settings.general.wslSaveError')}
            </p>
          ) : null}
        </div>

        <div className="flex h-8 min-w-14 items-center justify-end gap-2">
          {saving ? (
            <LoaderCircle
              role="status"
              aria-label={t('settings.general.wslSaving')}
              className="h-4 w-4 animate-spin text-muted-foreground"
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
                setConfirmOpen(true);
                return;
              }
              void changeSetting(nextEnabled);
            }}
          />
        </div>
      </section>

      <AlertDialog open={confirmOpen} onOpenChange={(open) => { if (!saving) setConfirmOpen(open); }}>
        <AlertDialogContent dismissible={!saving} aria-busy={saving}>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.general.wslDisableTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('settings.general.wslDisableDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={saving}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              disabled={saving}
              onClick={(event) => {
                event.preventDefault();
                void changeSetting(false);
              }}
            >
              {t('settings.general.wslDisableConfirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
