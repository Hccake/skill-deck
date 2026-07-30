import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Clock3, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';
import { getConfig, saveConfig, type SkillDeckConfig } from '@/hooks/useTauriApi';

const DEFAULT_TIMEOUT_SECS = 120;
const MIN_TIMEOUT_SECS = 30;
const MAX_TIMEOUT_SECS = 3600;
const PRESETS = [60, 120, 300, 600] as const;

type TimeoutOption = `${typeof PRESETS[number]}` | 'custom';

function normalizeTimeout(value: number | undefined): number {
  if (!value || Number.isNaN(value)) {
    return DEFAULT_TIMEOUT_SECS;
  }

  return Math.min(Math.max(Math.trunc(value), MIN_TIMEOUT_SECS), MAX_TIMEOUT_SECS);
}

function getOptionForTimeout(value: number): TimeoutOption {
  return PRESETS.includes(value as typeof PRESETS[number]) ? `${value}` as TimeoutOption : 'custom';
}

function presetLabel(seconds: typeof PRESETS[number], t: ReturnType<typeof useTranslation>['t']): string {
  switch (seconds) {
    case 60:
      return t('settings.cloneTimeout.presets.60');
    case 120:
      return t('settings.cloneTimeout.presets.120');
    case 300:
      return t('settings.cloneTimeout.presets.300');
    case 600:
      return t('settings.cloneTimeout.presets.600');
    default:
      return `${seconds}s`;
  }
}

export function GitCloneTimeoutSection() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<SkillDeckConfig | null>(null);
  const [currentTimeoutSecs, setCurrentTimeoutSecs] = useState(DEFAULT_TIMEOUT_SECS);
  const [selectedOption, setSelectedOption] = useState<TimeoutOption>('120');
  const [customValue, setCustomValue] = useState(String(DEFAULT_TIMEOUT_SECS));
  const [validationError, setValidationError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (savedTimerRef.current) {
        clearTimeout(savedTimerRef.current);
      }
    };
  }, []);

  const flashSaved = useCallback(() => {
    if (savedTimerRef.current) {
      clearTimeout(savedTimerRef.current);
    }
    setSaved(true);
    savedTimerRef.current = setTimeout(() => {
      setSaved(false);
      savedTimerRef.current = null;
    }, 1200);
  }, []);

  useEffect(() => {
    async function loadTimeoutConfig() {
      try {
        setLoading(true);
        const nextConfig = await getConfig();
        const nextTimeout = normalizeTimeout(nextConfig.gitCloneTimeoutSecs);
        setConfig(nextConfig);
        setCurrentTimeoutSecs(nextTimeout);
        setSelectedOption(getOptionForTimeout(nextTimeout));
        setCustomValue(String(nextTimeout));
      } catch (error) {
        console.error('Failed to load git clone timeout config:', error);
        setConfig(null);
        setCurrentTimeoutSecs(DEFAULT_TIMEOUT_SECS);
        setSelectedOption('120');
        setCustomValue(String(DEFAULT_TIMEOUT_SECS));
      } finally {
        setLoading(false);
      }
    }

    void loadTimeoutConfig();
  }, []);

  const persistTimeout = useCallback(async (
    nextTimeoutSecs: number,
    options?: {
      selectedOptionOnSuccess?: TimeoutOption;
      revertToCurrentOnError?: boolean;
    }
  ) => {
    const normalized = normalizeTimeout(nextTimeoutSecs);

    try {
      setSaving(true);
      setSaveError(null);
      const baseConfig = config ?? await getConfig();
      const nextConfig: SkillDeckConfig = {
        ...baseConfig,
        gitCloneTimeoutSecs: normalized,
      };

      await saveConfig(nextConfig);
      setConfig(nextConfig);
      setCurrentTimeoutSecs(normalized);
      setSelectedOption(options?.selectedOptionOnSuccess ?? getOptionForTimeout(normalized));
      setCustomValue(String(normalized));
      setValidationError(null);
      flashSaved();
    } catch (error) {
      console.error('Failed to save git clone timeout config:', error);
      if (options?.revertToCurrentOnError) {
        setSelectedOption(getOptionForTimeout(currentTimeoutSecs));
      }
      setSaveError(t('settings.cloneTimeout.saveError'));
    } finally {
      setSaving(false);
    }
  }, [config, currentTimeoutSecs, flashSaved, t]);

  const handlePresetClick = useCallback((seconds: typeof PRESETS[number]) => {
    void persistTimeout(seconds, {
      selectedOptionOnSuccess: `${seconds}` as TimeoutOption,
      revertToCurrentOnError: true,
    });
  }, [persistTimeout]);

  const handleCustomSave = useCallback(() => {
    const parsed = Number(customValue);

    if (!Number.isFinite(parsed)) {
      setValidationError(t('settings.cloneTimeout.errors.invalidNumber'));
      return;
    }

    if (parsed < MIN_TIMEOUT_SECS) {
      setValidationError(t('settings.cloneTimeout.errors.tooSmall'));
      return;
    }

    if (parsed > MAX_TIMEOUT_SECS) {
      setValidationError(t('settings.cloneTimeout.errors.tooLarge'));
      return;
    }

    const normalized = normalizeTimeout(parsed);
    void persistTimeout(parsed, {
      selectedOptionOnSuccess: getOptionForTimeout(normalized),
    });
  }, [customValue, persistTimeout, t]);

  if (loading) {
    return (
      <section className="px-4 py-4 sm:px-5">
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <Skeleton className="size-8 rounded-md" />
            <div className="space-y-1.5">
              <Skeleton className="h-4 w-32" />
              <Skeleton className="h-3 w-64 max-w-[200px] sm:max-w-none" />
            </div>
          </div>
          <Skeleton className="h-8 w-[110px] rounded-md" />
        </div>
      </section>
    );
  }

  const statusMessage = saveError ?? validationError ?? (saved ? t('settings.cloneTimeout.saved') : null);

  return (
    <section className="px-4 py-4 sm:px-5">
      <div className="flex items-start sm:items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-md bg-muted/70">
            <Clock3 className="size-4 text-muted-foreground" aria-hidden="true" />
          </div>
          <div className="min-w-0">
            <h2 className="text-sm font-medium text-foreground">
              {t('settings.cloneTimeout.title')}
            </h2>
            <p className="text-xs text-muted-foreground">
              {t('settings.cloneTimeout.description')}
            </p>
          </div>
        </div>

        <div
          data-testid="clone-timeout-meta"
          className="relative flex shrink-0 items-center gap-1.5"
        >
          {/* 预设值反馈 (仅在非自定义模式下可见) */}
          <div
            className={cn(
              'absolute right-full mr-2 flex items-center gap-1 text-xs font-medium transition-opacity duration-300',
              saveError
                ? 'text-destructive opacity-100'
                : (saved && selectedOption !== 'custom')
                  ? 'text-success opacity-100'
                  : 'opacity-0 pointer-events-none'
            )}
          >
            {!saveError ? <Check className="h-3.5 w-3.5" /> : null}
            <span className="hidden sm:inline whitespace-nowrap">
              {saveError ?? t('settings.cloneTimeout.saved')}
            </span>
          </div>

          <Select
            value={selectedOption}
            onValueChange={(val: TimeoutOption) => {
              if (val !== 'custom') {
                void handlePresetClick(Number(val) as typeof PRESETS[number]);
              } else {
                setSelectedOption(val);
                setValidationError(null);
                setSaveError(null);
              }
            }}
            disabled={saving}
          >
            <SelectTrigger size="sm" className="w-[110px] h-8 bg-background">
              <SelectValue placeholder={presetLabel(120, t)} />
            </SelectTrigger>
            <SelectContent>
              {PRESETS.map((seconds) => (
                <SelectItem key={seconds} value={`${seconds}`}>
                  {presetLabel(seconds, t)}
                </SelectItem>
              ))}
              <SelectItem value="custom">
                {t('settings.cloneTimeout.custom')}
              </SelectItem>
            </SelectContent>
          </Select>

          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={saving || currentTimeoutSecs === DEFAULT_TIMEOUT_SECS}
            className={cn(
              "h-8 w-8 rounded-md p-0 transition-all duration-300 cursor-pointer",
              currentTimeoutSecs === DEFAULT_TIMEOUT_SECS
                ? "opacity-0 scale-75 pointer-events-none"
                : "opacity-100 text-muted-foreground hover:bg-accent hover:text-foreground scale-100"
            )}
            onClick={() => {
              void persistTimeout(DEFAULT_TIMEOUT_SECS);
            }}
            title={t('settings.cloneTimeout.restoreDefault')}
          >
            <RotateCcw className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {selectedOption === 'custom' ? (
        <div className="mt-3 animate-in slide-in-from-top-2 fade-in duration-200">
          <div
            data-testid="clone-timeout-advanced"
            className="flex items-center gap-2 ml-9"
          >
            <label htmlFor="git-clone-timeout-custom" className="sr-only">
              {t('settings.cloneTimeout.customLabel')}
            </label>
            <div className="relative w-40 sm:w-48">
              <Input
                id="git-clone-timeout-custom"
                aria-label={t('settings.cloneTimeout.customLabel')}
                inputMode="numeric"
                value={customValue}
                className="h-8 rounded-md bg-background pr-12 text-sm shadow-none focus-visible:ring-1 focus-visible:ring-primary/30"
                onChange={(event) => {
                  setCustomValue(event.target.value);
                  setValidationError(null);
                  setSaveError(null);
                }}
              />
              <span className="pointer-events-none absolute inset-y-0 right-2.5 flex items-center text-xs text-muted-foreground">
                {t('settings.cloneTimeout.secondsUnit')}
              </span>
            </div>
            <Button
              type="button"
              size="sm"
              disabled={saving}
              onClick={handleCustomSave}
              className="h-8 rounded-md px-4 text-xs cursor-pointer shadow-sm"
            >
              {t('settings.cloneTimeout.saveButton')}
            </Button>

            {/* 内联状态信息 (仅在自定义模式下可见) */}
            <div
              className={cn(
                'flex items-center gap-1.5 text-xs font-medium ml-2 transition-all duration-300',
                saveError || validationError
                  ? 'text-destructive'
                  : saved
                    ? 'text-success'
                    : 'text-muted-foreground',
                statusMessage || selectedOption === 'custom' ? 'opacity-100 translate-x-0' : 'opacity-0 -translate-x-2'
              )}
            >
              {saved ? <Check className="h-3.5 w-3.5" /> : null}
              <span className="truncate max-w-[150px] sm:max-w-[200px]">
                {statusMessage
                  ?? (selectedOption === 'custom'
                    ? t('settings.cloneTimeout.customHint')
                    : '\u00A0')}
              </span>
            </div>
          </div>
        </div>
      ) : null}
    </section>
  );
}
