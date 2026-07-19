import { useTranslation } from 'react-i18next';
import type { AppError, EnvironmentInfo, EnvironmentRef } from '@/bindings';
import { environmentKey } from '@/stores/environment';
import type { EnvironmentDiscoveryState } from '@/stores/environment';
import { formatAppError } from '@/utils/format-app-error';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

interface EnvironmentSelectProps {
  environments: EnvironmentInfo[];
  value: EnvironmentRef;
  onChange: (environment: EnvironmentRef) => void | Promise<void>;
  className?: string;
  disabled?: boolean;
  discoveryState?: EnvironmentDiscoveryState;
  discoveryError?: AppError | null;
  connectionErrors?: Record<string, AppError | null>;
  pendingEnvironment?: EnvironmentRef | null;
  onRetryDiscovery?: () => void | Promise<void>;
  onRetryConnection?: (environment: EnvironmentRef) => void | Promise<void>;
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
  discoveryState = 'ready',
  discoveryError = null,
  connectionErrors = {},
  pendingEnvironment = null,
  onRetryDiscovery,
  onRetryConnection,
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
  const failedEntry = environments.find(
    (entry) => connectionErrors[environmentKey(entry.environment)] !== null
      && connectionErrors[environmentKey(entry.environment)] !== undefined,
  );
  const connectionError = failedEntry
    ? connectionErrors[environmentKey(failedEntry.environment)]
    : null;

  if (!showSelect && !discoveryError && !pendingEntry && !connectionError) return null;

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
          disabled={disabled || pendingEnvironment !== null || selectedEntry?.status === 'connecting'}
        >
          <SelectTrigger
            aria-label={t('context.environmentLabel')}
            aria-busy={pendingEnvironment !== null || selectedEntry?.status === 'connecting'}
            className={className ?? 'w-full'}
          >
            <SelectValue />
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

      {pendingEntry ? (
        <div role="status" aria-live="polite" className="text-xs text-muted-foreground">
          {t('context.environmentConnectingTo', { environment: pendingEntry.displayName })}
        </div>
      ) : discoveryState === 'error' && discoveryError ? (
        <Alert role="status" aria-live="polite" className="py-2.5">
          <AlertTitle>{t('context.environmentDiscoveryFailed')}</AlertTitle>
          <AlertDescription>
            <p className="text-xs">{formatAppError(discoveryError, t)}</p>
          {onRetryDiscovery ? (
            <Button
              variant="link"
              size="sm"
              className="h-auto p-0 text-xs"
              onClick={() => void onRetryDiscovery()}
            >
              {t('context.environmentRetry')}
            </Button>
          ) : null}
          </AlertDescription>
        </Alert>
      ) : failedEntry && connectionError ? (
        <Alert role="status" aria-live="polite" className="py-2.5">
          <AlertTitle>
            {t('context.environmentConnectionFailed', { environment: failedEntry.displayName })}
          </AlertTitle>
          <AlertDescription>
            <p className="text-xs">{formatAppError(connectionError, t)}</p>
          {onRetryConnection ? (
            <Button
              variant="link"
              size="sm"
              className="h-auto p-0 text-xs"
              onClick={() => void onRetryConnection(failedEntry.environment)}
            >
              {t('context.environmentRetryNamed', { environment: failedEntry.displayName })}
            </Button>
          ) : null}
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}
