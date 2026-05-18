import { useTranslation } from 'react-i18next';
import { Moon, Sun } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useSettingsStore } from '@/stores/settings';
import type { Locale, Theme } from '@/stores/settings';
import { cn } from '@/lib/utils';

const THEME_OPTIONS: Array<{ value: Theme; icon: typeof Sun; labelKey: string }> = [
  { value: 'light', icon: Sun, labelKey: 'theme.light' },
  { value: 'dark', icon: Moon, labelKey: 'theme.dark' },
];

const LOCALE_OPTIONS: Array<{ value: Locale; label: string }> = [
  { value: 'zh-CN', label: '简体中文' },
  { value: 'en', label: 'English' },
];

export function GeneralTab() {
  const { t } = useTranslation();
  const { theme, setTheme, locale, setLocale } = useSettingsStore();

  return (
    <div className="space-y-5">
      <header className="space-y-1">
        <h2 className="text-lg font-semibold tracking-tight text-foreground">
          {t('settings.general.title')}
        </h2>
        <p className="mt-1 text-sm text-muted-foreground">
          {t('settings.general.description')}
        </p>
      </header>

      <div className="divide-y divide-border/60 overflow-hidden rounded-lg border border-border/60 bg-background">
        <section className="grid gap-4 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
          <div className="flex min-w-0 items-start gap-3">
            <div className="space-y-1">
              <p className="text-sm font-medium text-foreground">
                {t('settings.general.appearanceTitle')}
              </p>
              <p className="text-xs leading-5 text-muted-foreground">
                {t('settings.general.appearanceDescription')}
              </p>
            </div>
          </div>

          <div className="relative grid grid-cols-2 rounded-md border border-border/60 bg-muted/25 p-1">
            <div
              className={cn(
                "absolute inset-y-1 left-1 w-[calc(50%-4px)] rounded bg-background shadow-sm transition-transform duration-200 ease-out",
                theme === 'dark' ? "translate-x-full" : "translate-x-0"
              )}
            />
            {THEME_OPTIONS.map((option) => {
              const Icon = option.icon;
              const selected = theme === option.value;

              return (
                <Button
                  key={option.value}
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setTheme(option.value)}
                  className={cn(
                    'relative z-10 h-8 gap-1.5 px-3 text-xs',
                    selected
                      ? 'text-foreground hover:bg-transparent'
                      : 'text-muted-foreground hover:text-foreground hover:bg-transparent'
                  )}
                >
                  <Icon className="h-3.5 w-3.5" />
                  {t(option.labelKey)}
                </Button>
              );
            })}
          </div>
        </section>

        <section className="grid gap-4 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
          <div className="flex min-w-0 items-start gap-3">
            <div className="space-y-1">
              <p className="text-sm font-medium text-foreground">
                {t('settings.general.languageTitle')}
              </p>
              <p className="text-xs leading-5 text-muted-foreground">
                {t('settings.general.languageDescription')}
              </p>
            </div>
          </div>

          <div className="relative grid grid-cols-2 rounded-md border border-border/60 bg-muted/25 p-1">
            <div
              className={cn(
                "absolute inset-y-1 left-1 w-[calc(50%-4px)] rounded bg-background shadow-sm transition-transform duration-200 ease-out",
                locale === 'en' ? "translate-x-full" : "translate-x-0"
              )}
            />
            {LOCALE_OPTIONS.map((option) => {
              const selected = locale === option.value;

              return (
                <Button
                  key={option.value}
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setLocale(option.value)}
                  className={cn(
                    'relative z-10 h-8 px-3 text-xs',
                    selected
                      ? 'text-foreground hover:bg-transparent'
                      : 'text-muted-foreground hover:text-foreground hover:bg-transparent'
                  )}
                >
                  {option.label}
                </Button>
              );
            })}
          </div>
        </section>
      </div>
    </div>
  );
}
