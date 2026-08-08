import type { EnvironmentInfo, EnvironmentRef } from '@/bindings';

type Translate = (
  key: string,
  values?: Partial<Record<string, string>>,
) => string;

export function environmentDisplayName(
  entry: Pick<EnvironmentInfo, 'environment' | 'displayName'>,
  t: Translate,
): string {
  return entry.environment.kind === 'wsl'
    ? t('context.environmentWslName', { environment: entry.displayName })
    : entry.displayName;
}

export function environmentRefDisplayName(
  environment: EnvironmentRef,
  nativeDisplayName: string | undefined,
  t: Translate,
): string {
  return environment.kind === 'wsl'
    ? t('context.environmentWslName', { environment: environment.distro_name })
    : nativeDisplayName ?? '';
}
