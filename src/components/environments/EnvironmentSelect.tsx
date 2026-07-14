import { useTranslation } from 'react-i18next';
import type { AppError, EnvironmentInfo, EnvironmentRef } from '@/bindings';
import { environmentKey } from '@/stores/environment';
import type { EnvironmentDiscoveryState } from '@/stores/environment';
import { formatAppError } from '@/utils/format-app-error';

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
        <select
          aria-label={t('context.environmentLabel')}
          value={environmentKey(value)}
          onChange={(event) => handleChange(event.target.value)}
          disabled={disabled || pendingEnvironment !== null || selectedEntry?.status === 'connecting'}
          aria-busy={pendingEnvironment !== null || selectedEntry?.status === 'connecting'}
          className={className ?? 'h-9 w-full rounded-md border border-border/60 bg-background px-3 text-sm text-foreground'}
        >
          {environments.map((entry) => {
            const status = statusLabel(entry.status, t);
            return (
              <option
                key={environmentKey(entry.environment)}
                value={environmentKey(entry.environment)}
                title={entry.displayName}
              >
                {status ? `${entry.displayName} · ${status}` : entry.displayName}
              </option>
            );
          })}
        </select>
      ) : null}

      {pendingEntry ? (
        <div role="status" aria-live="polite" className="text-xs text-muted-foreground">
          {t('context.environmentConnectingTo', { environment: pendingEntry.displayName })}
        </div>
      ) : discoveryState === 'error' && discoveryError ? (
        <div className="space-y-1 text-xs text-muted-foreground">
          <div role="status" aria-live="polite" className="space-y-1">
            <p>{t('context.environmentDiscoveryFailed')}</p>
            <p className="text-[10px] opacity-80">{formatAppError(discoveryError, t)}</p>
          </div>
          {onRetryDiscovery ? (
            <button
              type="button"
              className="text-primary hover:underline focus-visible:underline"
              onClick={() => void onRetryDiscovery()}
            >
              {t('context.environmentRetry')}
            </button>
          ) : null}
        </div>
      ) : failedEntry && connectionError ? (
        <div className="space-y-1 text-xs text-muted-foreground">
          <div role="status" aria-live="polite" className="space-y-1">
            <p>{t('context.environmentConnectionFailed', { environment: failedEntry.displayName })}</p>
            <p className="text-[10px] opacity-80">{formatAppError(connectionError, t)}</p>
          </div>
          {onRetryConnection ? (
            <button
              type="button"
              className="text-primary hover:underline focus-visible:underline"
              onClick={() => void onRetryConnection(failedEntry.environment)}
            >
              {t('context.environmentRetryNamed', { environment: failedEntry.displayName })}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
