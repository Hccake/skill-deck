import { useTranslation } from 'react-i18next';
import type { EnvironmentInfo, EnvironmentRef } from '@/bindings';
import { environmentKey } from '@/stores/environment';

interface EnvironmentSelectProps {
  environments: EnvironmentInfo[];
  value: EnvironmentRef;
  onChange: (environment: EnvironmentRef) => void | Promise<void>;
  className?: string;
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
}: EnvironmentSelectProps) {
  const { t } = useTranslation();

  if (environments.length <= 1) return null;

  const selectedEntry = environments.find(
    (entry) => environmentKey(entry.environment) === environmentKey(value),
  );

  const handleChange = (key: string) => {
    const environment = environments.find(
      (entry) => environmentKey(entry.environment) === key,
    )?.environment;
    if (environment) void onChange(environment);
  };

  return (
    <select
      aria-label={t('context.environmentLabel')}
      value={environmentKey(value)}
      onChange={(event) => handleChange(event.target.value)}
      disabled={selectedEntry?.status === 'connecting'}
      aria-busy={selectedEntry?.status === 'connecting'}
      className={className ?? 'h-9 w-full rounded-md border border-border/60 bg-background px-3 text-sm text-foreground'}
    >
      {environments.map((entry) => {
        const status = statusLabel(entry.status, t);
        return (
          <option
            key={environmentKey(entry.environment)}
            value={environmentKey(entry.environment)}
          >
            {status ? `${entry.displayName} · ${status}` : entry.displayName}
          </option>
        );
      })}
    </select>
  );
}
