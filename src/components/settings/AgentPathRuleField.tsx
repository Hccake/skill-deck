import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { CustomPathBase, CustomPathSpec } from '@/bindings';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { cn } from '@/lib/utils';

export type AgentPathLocation = CustomPathBase | 'absolute';

interface AgentPathRuleFieldProps {
  id: string;
  name: string;
  value: CustomPathSpec;
  allowedLocations: readonly AgentPathLocation[];
  locationAriaLabel: string;
  pathAriaLabel?: string;
  describedBy?: string;
  invalid?: boolean;
  disabled: boolean;
  required?: boolean;
  inputRef?: React.Ref<HTMLInputElement>;
  onChange: (value: CustomPathSpec) => void;
}

function locationOf(value: CustomPathSpec): AgentPathLocation {
  return value.kind === 'absolute' ? 'absolute' : value.base;
}

function pathOf(value: CustomPathSpec): string {
  return value.kind === 'absolute' ? value.path : value.relativePath;
}

export function AgentPathRuleField({
  id,
  name,
  value,
  allowedLocations,
  locationAriaLabel,
  pathAriaLabel,
  describedBy,
  invalid = false,
  disabled,
  required = false,
  inputRef,
  onChange,
}: AgentPathRuleFieldProps) {
  const { t } = useTranslation();
  const drafts = useRef({
    based: value.kind === 'based' ? value.relativePath : '',
    absolute: value.kind === 'absolute' ? value.path : '',
  });
  const location = locationOf(value);
  const showsLocationSelect = allowedLocations.length > 1;
  const prefix = location === 'home'
    ? '~/'
    : location === 'configHome'
      ? '~/.config/'
      : location === 'project'
        ? t('settings.agents.project.pathPrefix')
        : null;

  useEffect(() => {
    if (value.kind === 'absolute') drafts.current.absolute = value.path;
    else drafts.current.based = value.relativePath;
  }, [value]);

  const setLocation = (nextLocation: AgentPathLocation) => {
    if (value.kind === 'absolute') drafts.current.absolute = value.path;
    else drafts.current.based = value.relativePath;
    onChange(nextLocation === 'absolute'
      ? { kind: 'absolute', path: drafts.current.absolute }
      : { kind: 'based', base: nextLocation, relativePath: drafts.current.based });
  };

  const setPath = (path: string) => {
    if (value.kind === 'absolute') {
      drafts.current.absolute = path;
      onChange({ kind: 'absolute', path });
    } else {
      drafts.current.based = path;
      onChange({ ...value, relativePath: path });
    }
  };

  return (
    <div className={cn(
      'grid min-w-0 gap-2',
      showsLocationSelect && 'sm:grid-cols-[10rem_minmax(0,1fr)]',
    )}>
      {showsLocationSelect ? (
        <Select
          value={location}
          disabled={disabled}
          onValueChange={(nextLocation) => setLocation(nextLocation as AgentPathLocation)}
        >
          <SelectTrigger id={`${id}-location`} className="w-full" aria-label={locationAriaLabel}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {allowedLocations.map((allowedLocation) => (
              <SelectItem key={allowedLocation} value={allowedLocation}>
                {t(`settings.agents.pathLocations.${allowedLocation}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : null}

      <div className={cn(
        'flex min-w-0 rounded-md shadow-xs',
        prefix && 'focus-within:ring-[3px] focus-within:ring-ring/50',
      )}>
        {prefix ? (
          <span
            className="inline-flex shrink-0 items-center rounded-l-md border border-r-0 border-input bg-muted/50 px-3 text-sm text-muted-foreground"
            translate="no"
          >
            {prefix}
          </span>
        ) : null}
        <Input
          ref={inputRef}
          id={id}
          name={name}
          autoComplete="off"
          spellCheck={false}
          translate="no"
          value={pathOf(value)}
          disabled={disabled}
          required={required}
          aria-label={pathAriaLabel}
          aria-describedby={describedBy}
          aria-invalid={invalid}
          className={prefix ? 'rounded-l-none shadow-none focus-visible:ring-0' : undefined}
          placeholder={location === 'absolute'
            ? t('settings.agents.detection.absolutePlaceholder')
            : undefined}
          onChange={(event) => setPath(event.target.value)}
        />
      </div>
    </div>
  );
}
