import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';
import { Sun, Moon, Package, Settings, Check } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useSettingsStore } from '@/stores/settings';
import { cn } from '@/lib/utils';
import type { Locale } from '@/stores/settings';

// Hoisted outside component to avoid recreation on each render
const getNavLinkClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    'px-3.5 py-1.5 text-sm font-medium rounded-md transition-all duration-200',
    'flex items-center gap-1.5',
    isActive
      ? 'bg-foreground text-background shadow-sm'
      : 'text-muted-foreground hover:text-foreground'
  );

const LOCALE_OPTIONS: { value: Locale; code: string; label: string }[] = [
  { value: 'zh-CN', code: 'ZH', label: '简体中文' },
  { value: 'en', code: 'EN', label: 'English' },
];

export function Header() {
  const { t } = useTranslation();
  const { theme, toggleTheme, locale, setLocale } = useSettingsStore();

  return (
    <header className="flex h-14 items-center justify-between px-4 sm:px-6 border-b border-border/40 bg-background/80 backdrop-blur-sm">
      {/* Left: Logo + Brand */}
      <div className="flex items-center gap-2.5 min-w-[120px]">
        <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-gradient-to-br from-teal-500 to-teal-600 shadow-md shadow-primary/20">
          <span className="text-sm font-semibold text-white">S</span>
        </div>
        <span className="hidden sm:inline text-sm font-semibold text-foreground tracking-tight">
          {t('app.name')}
        </span>
      </div>

      {/* Center: Navigation Tabs */}
      <nav className="flex items-center gap-1 rounded-lg bg-muted p-1">
        <NavLink to="/" end className={getNavLinkClass}>
          <Package className="h-4 w-4 sm:hidden" />
          <span>{t('nav.skills')}</span>
        </NavLink>
        <NavLink to="/settings" className={getNavLinkClass}>
          <Settings className="h-4 w-4 sm:hidden" />
          <span>{t('nav.settings')}</span>
        </NavLink>
      </nav>

      {/* Right: Tool Buttons */}
      <div className="flex items-center gap-1 min-w-[120px] justify-end">
        {/* Language Selector */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="group h-8 w-8 cursor-pointer"
            >
              <span className="flex h-7 w-7 items-center justify-center rounded-full bg-muted text-xs font-semibold text-muted-foreground transition-colors group-hover:bg-muted-foreground/15 group-hover:text-foreground">
                {LOCALE_OPTIONS.find((o) => o.value === locale)?.code}
              </span>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {LOCALE_OPTIONS.map((option) => (
              <DropdownMenuItem
                key={option.value}
                onClick={() => setLocale(option.value)}
                className="cursor-pointer"
              >
                <span className="font-mono text-xs w-6">{option.code}</span>
                <span>{option.label}</span>
                {locale === option.value && (
                  <Check className="h-3.5 w-3.5 ml-auto text-primary" />
                )}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Theme Toggle */}
        <Button
          variant="ghost"
          size="icon"
          className="group h-8 w-8 cursor-pointer"
          onClick={toggleTheme}
          aria-label={t(`theme.${theme}`)}
          title={t(`theme.${theme}`)}
        >
          <span className="flex h-7 w-7 items-center justify-center rounded-full bg-muted text-muted-foreground transition-colors group-hover:bg-muted-foreground/15 group-hover:text-foreground">
            {theme === 'light' ? (
              <Sun className="h-4 w-4" />
            ) : (
              <Moon className="h-4 w-4" />
            )}
          </span>
        </Button>
      </div>
    </header>
  );
}
