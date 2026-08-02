import { ChevronDown, LoaderCircle, RefreshCw, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { EnvironmentPlatformIcon } from '@/components/environments/EnvironmentPlatformIcon';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useOptionalUnsavedChanges } from '@/lifecycle/unsaved-changes-context';
import { environmentKey, useEnvironmentStore } from '@/stores/environment';
import { useMutationStore } from '@/stores/mutation';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { formatAppError } from '@/utils/format-app-error';
import { toAppError } from '@/utils/to-app-error';
import type { EnvironmentInfo, EnvironmentRef } from '@/bindings';

function environmentDisplayName(
  entry: Pick<EnvironmentInfo, 'environment' | 'displayName'>,
  t: (key: string, values?: Record<string, unknown>) => string,
): string {
  return entry.environment.kind === 'wsl'
    ? t('context.environmentWslName', { environment: entry.displayName })
    : entry.displayName;
}

export function GlobalEnvironmentSwitcher() {
  const { t } = useTranslation();
  const unsavedChanges = useOptionalUnsavedChanges();
  const environments = useEnvironmentStore((state) => state.environments);
  const discoveryError = useEnvironmentStore((state) => state.discoveryError);
  const discover = useEnvironmentStore((state) => state.discover);
  const selectedEnvironment = useWorkspaceContextStore(
    (state) => state.selectedContext.environment,
  );
  const transition = useWorkspaceContextStore((state) => state.transition);
  const switchEnvironment = useWorkspaceContextStore((state) => state.switchEnvironment);
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const selectedKey = environmentKey(selectedEnvironment);
  const selectedEntry = environments.find(
    (entry) => environmentKey(entry.environment) === selectedKey,
  );
  const selectedLabel = selectedEntry
    ? environmentDisplayName(selectedEntry, t)
    : selectedEnvironment.kind === 'wsl'
      ? t('context.environmentWslName', { environment: selectedEnvironment.distro_name })
      : 'Host';
  const selectedConnectionError = selectedEntry
    && (selectedEntry.status === 'unavailable' || selectedEntry.status === 'error')
    ? selectedEntry.error
    : null;
  const pending = transition.kind !== 'idle';
  const disabled = pending || writeBlocked;
  const visible = environments.length > 1 || discoveryError !== null || selectedConnectionError !== null;

  if (!visible) return null;

  const runSwitch = (environment: EnvironmentRef) => {
    const action = async () => {
      try {
        await switchEnvironment(environment);
      } catch (error) {
        toast.error(formatAppError(toAppError(error), t));
      }
    };
    if (unsavedChanges) void unsavedChanges.guard(action);
    else void action();
  };

  const retryDiscovery = () => {
    void discover().catch((error) => {
      toast.error(formatAppError(toAppError(error), t));
    });
  };

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={disabled}
              aria-busy={pending}
              aria-label={t('context.environmentMenuLabel', { environment: selectedLabel })}
              className="h-8 max-w-40 cursor-pointer gap-1.5 px-2 text-muted-foreground hover:text-foreground sm:h-9 sm:px-2.5"
            >
              <EnvironmentPlatformIcon
                environment={selectedEnvironment}
                className="h-4 w-4 shrink-0"
              />
              <span className="hidden min-w-0 truncate lg:inline">{selectedLabel}</span>
              {pending ? (
                <LoaderCircle className="h-3.5 w-3.5 shrink-0 animate-spin" aria-hidden="true" />
              ) : selectedConnectionError || discoveryError ? (
                <TriangleAlert className="h-3.5 w-3.5 shrink-0 text-warning" aria-hidden="true" />
              ) : (
                <ChevronDown className="hidden h-3.5 w-3.5 shrink-0 lg:block" aria-hidden="true" />
              )}
              {pending ? (
                <span role="status" aria-live="polite" className="sr-only">
                  {t('context.environmentConnectingTo', { environment: selectedLabel })}
                </span>
              ) : null}
            </Button>
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent className="lg:hidden">{selectedLabel}</TooltipContent>
      </Tooltip>
      <DropdownMenuContent
        align="end"
        className="w-max min-w-(--radix-dropdown-menu-trigger-width) max-w-[calc(100vw-1rem)] sm:max-w-72"
      >
        <DropdownMenuLabel className="text-xs text-muted-foreground">
          {t('context.environmentLabel')}
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={selectedKey}
          onValueChange={(key) => {
            const target = environments.find(
              (entry) => environmentKey(entry.environment) === key,
            );
            if (target && key !== selectedKey) runSwitch(target.environment);
          }}
        >
          {environments.map((entry) => (
            <DropdownMenuRadioItem
              key={environmentKey(entry.environment)}
              value={environmentKey(entry.environment)}
              className="pl-2 [&>span:first-child]:hidden data-[state=checked]:bg-accent data-[state=checked]:font-medium data-[state=checked]:text-accent-foreground"
            >
              <EnvironmentPlatformIcon
                environment={entry.environment}
                className="h-4 w-4"
              />
              <span className="min-w-0 flex-1 truncate">
                {environmentDisplayName(entry, t)}
              </span>
              {entry.status === 'connecting' ? (
                <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
              ) : entry.status === 'unavailable' || entry.status === 'error' ? (
                <span className="text-xs text-muted-foreground">
                  {t('context.environmentUnavailable')}
                </span>
              ) : null}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>

        {selectedConnectionError && selectedEntry ? (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="space-y-1 whitespace-normal py-2">
              <span className="block text-xs font-medium text-destructive">
                {t('context.environmentConnectionFailed', { environment: selectedLabel })}
              </span>
              <span className="block text-xs font-normal text-muted-foreground">
                {formatAppError(selectedConnectionError, t)}
              </span>
            </DropdownMenuLabel>
            <DropdownMenuItem onSelect={() => runSwitch(selectedEntry.environment)}>
              <RefreshCw className="h-4 w-4" aria-hidden="true" />
              {t('context.environmentRetry')}
            </DropdownMenuItem>
          </>
        ) : null}

        {discoveryError ? (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="space-y-1 whitespace-normal py-2">
              <span className="block text-xs font-medium text-foreground">
                {t('context.environmentDiscoveryFailed')}
              </span>
              <span className="block text-xs font-normal text-muted-foreground">
                {formatAppError(discoveryError, t)}
              </span>
            </DropdownMenuLabel>
            <DropdownMenuItem onSelect={retryDiscovery}>
              <RefreshCw className="h-4 w-4" aria-hidden="true" />
              {t('context.environmentRetry')}
            </DropdownMenuItem>
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
