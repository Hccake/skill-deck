import { LoaderCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AppError, EnvironmentInfo, EnvironmentRef } from '@/bindings';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { environmentKey } from '@/stores/environment';
import { formatAppError } from '@/utils/format-app-error';

interface EnvironmentSelectProps {
  environments: EnvironmentInfo[];
  value: EnvironmentRef;
  onChange: (environment: EnvironmentRef) => void | Promise<void>;
  className?: string;
  disabled?: boolean;
  pendingEnvironment?: EnvironmentRef | null;
  discoveryError?: AppError | null;
}

function statusLabel(status: EnvironmentInfo['status'], t: (key: string) => string): string {
  switch (status) {
    case 'connecting':
      return t('context.environmentConnecting');
    case 'unavailable':
    case 'error':
      return t('context.environmentUnavailable');
    default:
      return '';
  }
}

export function EnvironmentSelect({
  environments,
  value,
  onChange,
  className,
  disabled = false,
  pendingEnvironment = null,
  discoveryError = null,
}: EnvironmentSelectProps) {
  const { t } = useTranslation();
  const showSelect = environments.length > 1;

  const selectedEntry = environments.find(
    (entry) => environmentKey(entry.environment) === environmentKey(value),
  );
  const pendingEntry = pendingEnvironment
    ? environments.find(
      (entry) => environmentKey(entry.environment) === environmentKey(pendingEnvironment),
    )
    : null;
  const pending = pendingEnvironment !== null || selectedEntry?.status === 'connecting';
  const failedEntry = environments.find((entry) => entry.error != null);
  const connectionError = failedEntry?.error ?? null;

  if (!showSelect && !discoveryError && !connectionError) return null;

  const handleChange = (key: string) => {
    const environment = environments.find(
      (entry) => environmentKey(entry.environment) === key,
    )?.environment;
    if (environment) void onChange(environment);
  };

  return (
    <div className="space-y-2">
      {showSelect ? (
        <Select
          value={environmentKey(value)}
          onValueChange={handleChange}
          disabled={disabled || pending}
        >
          <SelectTrigger
            aria-label={t('context.environmentLabel')}
            aria-busy={pending}
            className={className ?? 'w-full'}
          >
            <SelectValue />
            {pending ? (
              <>
                <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" aria-hidden="true" />
                <span role="status" aria-live="polite" className="sr-only">
                  {pendingEntry
                    ? t('context.environmentConnectingTo', { environment: pendingEntry.displayName })
                    : t('context.environmentConnecting')}
                </span>
              </>
            ) : null}
          </SelectTrigger>
          <SelectContent position="popper">
            {environments.map((entry) => {
              const status = statusLabel(entry.status, t);
              return (
                <SelectItem
                  key={environmentKey(entry.environment)}
                  value={environmentKey(entry.environment)}
                  title={entry.displayName}
                >
                  {status ? `${entry.displayName} · ${status}` : entry.displayName}
                </SelectItem>
              );
            })}
          </SelectContent>
        </Select>
      ) : null}

      {discoveryError ? (
        <Alert role="status" aria-live="polite" className="py-2.5">
          <AlertTitle>{t('context.environmentDiscoveryFailed')}</AlertTitle>
          <AlertDescription>
            <p className="text-xs">{formatAppError(discoveryError, t)}</p>
          </AlertDescription>
        </Alert>
      ) : null}

      {failedEntry && connectionError ? (
        <Alert role="status" aria-live="polite" className="py-2.5">
          <AlertTitle>
            {t('context.environmentConnectionFailed', { environment: failedEntry.displayName })}
          </AlertTitle>
          <AlertDescription>
            <p className="text-xs">{formatAppError(connectionError, t)}</p>
            <Button
              variant="link"
              size="sm"
              className="h-auto p-0 text-xs"
              onClick={() => void onChange(failedEntry.environment)}
            >
              {t('context.environmentRetryNamed', { environment: failedEntry.displayName })}
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}
